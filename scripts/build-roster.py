#!/usr/bin/env python3
"""Build the static site from the administration's V2 roster and verified photos.

The roster section of index.html is rebuilt between <!-- ROSTER:START --> and
<!-- ROSTER:END --> markers. The members count is updated between
<!-- MEMBER_COUNT:START --> and <!-- MEMBER_COUNT:END -->. members.txt is a
Ham2K PoLo callsign notes file (auto-generated, not manually edited).

tree.html, nearby.html and outbreak.html each get a JSON payload injected
between their own START/END markers (downline, geo, and outbreak data).
"""

import argparse
import json
import os
import re
import shutil
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

from roster_assets import annotate_public_assets
from roster_input import parse_roster_json, read_roster_json
from site_contract import GAME_DIR, PUBLIC_FILES, PUBLIC_ROSTER_PATH, ROSTER_START, validate_dist

REPO_ROOT = Path(__file__).resolve().parent.parent
INDEX_PATH = REPO_ROOT / "index.html"
TREE_PATH = REPO_ROOT / "tree.html"
NEARBY_PATH = REPO_ROOT / "nearby.html"
OUTBREAK_PATH = REPO_ROOT / "outbreak.html"
NEW_BADGE_LIMIT = 3  # last N members get the "NEW!!" badge

# Roster layout on index.html: "list" is the searchable list/mugshot wall;
# "classic" is the original card wall, kept as a rollback. The deploy workflow
# sets ROSTER_LAYOUT from the repository variable of the same name, so the old
# roster can return without a code change. This is display only and separate
# from the roster data contract.
ROSTER_LAYOUTS = ("list", "classic")
DEFAULT_ROSTER_LAYOUT = "list"
ROSTER_BLOCK_SIZE = 100  # members per numbered block in the list layout
FAME_RECRUITERS = 4  # top recruiters in the list layout's "Most Wanted" strip
FAME_NEWEST = 5  # newest members in the list layout's "Most Wanted" strip

# The 50 states + DC drawn in the index.html SVG map.
US_MAP_STATES = frozenset(
    """
    AL AK AZ AR CA CO CT DE DC FL GA HI ID IL IN IA KS KY LA ME MD MA MI MN
    MS MO MT NE NV NH NJ NM NY NC ND OH OK OR PA RI SC SD TN TX UT VT VA WA
    WV WI WY
    """.split()
)

# Country values (DXCC-style entity names) that belong on the US map
# rather than in the international DX box. Compared lowercase.
US_DXCC_NAMES = {"united states", "united states of america", "usa", "alaska", "hawaii"}

# Lowercase DXCC entity name -> flag emoji for the DX box.
# Anything missing falls back to the 🌍 globe.
COUNTRY_FLAGS = {
    "canada": "🇨🇦", "mexico": "🇲🇽", "england": "🏴󠁧󠁢󠁥󠁮󠁧󠁿", "scotland": "🏴󠁧󠁢󠁳󠁣󠁴󠁿",
    "wales": "🏴󠁧󠁢󠁷󠁬󠁳󠁿", "northern ireland": "🇬🇧", "ireland": "🇮🇪", "france": "🇫🇷",
    "fed. rep. of germany": "🇩🇪", "germany": "🇩🇪", "italy": "🇮🇹", "spain": "🇪🇸",
    "portugal": "🇵🇹", "netherlands": "🇳🇱", "belgium": "🇧🇪", "switzerland": "🇨🇭",
    "austria": "🇦🇹", "sweden": "🇸🇪", "norway": "🇳🇴", "denmark": "🇩🇰",
    "finland": "🇫🇮", "iceland": "🇮🇸", "poland": "🇵🇱", "czech republic": "🇨🇿",
    "slovak republic": "🇸🇰", "hungary": "🇭🇺", "romania": "🇷🇴", "bulgaria": "🇧🇬",
    "greece": "🇬🇷", "croatia": "🇭🇷", "slovenia": "🇸🇮", "ukraine": "🇺🇦",
    "european russia": "🇷🇺", "asiatic russia": "🇷🇺", "estonia": "🇪🇪",
    "latvia": "🇱🇻", "lithuania": "🇱🇹", "japan": "🇯🇵", "republic of korea": "🇰🇷",
    "south korea": "🇰🇷", "china": "🇨🇳", "taiwan": "🇹🇼", "hong kong": "🇭🇰",
    "philippines": "🇵🇭", "indonesia": "🇮🇩", "thailand": "🇹🇭", "vietnam": "🇻🇳",
    "india": "🇮🇳", "israel": "🇮🇱", "turkey": "🇹🇷", "united arab emirates": "🇦🇪",
    "south africa": "🇿🇦", "egypt": "🇪🇬", "kenya": "🇰🇪", "nigeria": "🇳🇬",
    "morocco": "🇲🇦", "australia": "🇦🇺", "new zealand": "🇳🇿", "brazil": "🇧🇷",
    "argentina": "🇦🇷", "chile": "🇨🇱", "colombia": "🇨🇴", "peru": "🇵🇪",
    "uruguay": "🇺🇾", "paraguay": "🇵🇾", "bolivia": "🇧🇴", "ecuador": "🇪🇨",
    "venezuela": "🇻🇪", "costa rica": "🇨🇷", "panama": "🇵🇦", "guatemala": "🇬🇹",
    "honduras": "🇭🇳", "nicaragua": "🇳🇮", "el salvador": "🇸🇻", "belize": "🇧🇿",
    "dominican republic": "🇩🇴", "cuba": "🇨🇺", "jamaica": "🇯🇲", "bahamas": "🇧🇸",
    "trinidad & tobago": "🇹🇹", "puerto rico": "🇵🇷", "us virgin islands": "🇻🇮",
    "guam": "🇬🇺",
}


def member_map_bucket(member: dict) -> tuple[str, str] | None:
    """Where a member lands on the territory map.

    Returns ("state", "UT") for members on the US map, ("dx", "Canada") for
    international members, or None when there's nothing to plot. A subdivision
    code belongs on the US map only when the country agrees.
    """
    state = member.get("state")
    country = (member.get("country") or "").strip()
    is_us = not country or country.lower() in US_DXCC_NAMES
    if is_us and state in US_MAP_STATES:
        return ("state", state)
    if country and not is_us:
        return ("dx", country)
    return None


def html_escape(text: str) -> str:
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def qth_location(qth: str) -> tuple[str | None, str | None]:
    """Reviewed QTH -> (subdivision code, country).

    US states give ("UT", "United States"). Canadian provinces are reviewed
    as bare names ("Alberta"), so they give ("AB", "Canada") rather than
    treating the province as a country: the territory map files them under
    Canada. Anything else is a country.
    """
    text = qth.strip()
    if not text:
        return None, None
    code = STATE_CODES_BY_NAME.get(text.lower())
    if code is None and text.upper() in US_MAP_STATES:
        code = text.upper()
    if code:
        return code, "United States"
    province = PROVINCE_CODES_BY_NAME.get(text.lower())
    if province:
        return province, "Canada"
    return None, text


def canonicalize_countries(members: list[dict]) -> None:
    """Spell each country one way, so "POLAND" and "Poland" share a territory.

    Reviewed QTH text is kept as published; only the derived country used for
    map, roster and outbreak grouping is unified. The most common spelling
    wins, then one that is not all capitals or all lowercase, then the first
    alphabetically, so the choice is deterministic.
    """
    spellings: dict[str, dict[str, int]] = {}
    for member in members:
        country = member.get("country")
        if country:
            counts = spellings.setdefault(country.casefold(), {})
            counts[country] = counts.get(country, 0) + 1
    canonical = {
        key: min(counts, key=lambda name: (-counts[name], name.isupper() or name.islower(), name))
        for key, counts in spellings.items()
    }
    for member in members:
        if member.get("country"):
            member["country"] = canonical[member["country"].casefold()]


def territory_og_badge_html(member: dict) -> str:
    """Render stored OG regions independently of the holder's current QTH."""
    regions = member["og_regions"]
    if not regions:
        return ""
    labels = [region["region_label"] for region in regions]
    title = html_escape(", ".join(f"{label} OG" for label in labels))
    label = html_escape(" / ".join(labels))
    return f'\n                    <div class="state-og-badge" title="{title}">{label} OG</div>'


def mugshot_inner_html(member: dict) -> str:
    path = member.get("mugshot_path")
    if path:
        callsign = html_escape(member["callsign"])
        src = html_escape(path)
        return (
            f'<img class="mugshot-photo" src="{src}" alt="{callsign} profile photo" loading="lazy">'
        )
    return (
        '<div class="mugshot-placeholder">\n'
        '                            <span class="icon">👤</span>\n'
        '                            PHOTO<br>PENDING\n'
        '                        </div>'
    )


def render_founder_card(member: dict) -> str:
    callsign = html_escape(member["callsign"])
    name = html_escape(member["name"])
    number = f"BKG #{member['number']:03d}"
    territory_og_html = territory_og_badge_html(member)
    mugshot_inner = mugshot_inner_html(member)
    return f"""                <!-- FOUNDER - {callsign} - THE OG BRASS POUNDER -->
                <div class="member-card founder-card">{territory_og_html}
                    <div class="founder-badge">👑 GODFATHER 👑</div>
                    <div class="founder-flames"></div>
                    <div class="mugshot founder-mugshot">
                        {mugshot_inner}
                        <div class="founder-glow"></div>
                    </div>
                    <div class="member-info founder-info">
                        <a href="https://www.qrz.com/db/{callsign}" target="_blank" class="member-callsign founder-callsign">{callsign}</a>
                        <div class="founder-title">🤜 THE OG BRASS POUNDER 🤛</div>
                        <div class="member-number">{number}</div>
                        <div class="member-name">{name}</div>
                        <div class="founder-quote">"2m CW or BUST"</div>
                    </div>
                    <div class="founder-sparks"></div>
                </div>"""


def render_member_card(member: dict, *, is_new: bool) -> str:
    callsign = html_escape(member["callsign"])
    name = html_escape(member["name"])
    number = f"BKG #{member['number']:03d}"
    badge_html = ""
    if is_new:
        badge_html = '\n                    <div class="new-badge">NEW!!</div>'
    badge_html += territory_og_badge_html(member)
    mugshot_inner = mugshot_inner_html(member)
    return f"""                <!-- {callsign} -->
                <div class="member-card">{badge_html}
                    <div class="mugshot">
                        {mugshot_inner}
                    </div>
                    <div class="member-info">
                        <a href="https://www.qrz.com/db/{callsign}" target="_blank" class="member-callsign">{callsign}</a>
                        <div class="member-number">{number}</div>
                        <div class="member-name">{name}</div>
                    </div>
                </div>"""


def render_ghost_card(next_number: int) -> str:
    number = f"BKG #{next_number:03d}"
    return f"""                <!-- Placeholder for future members -->
                <div class="member-card ghost">
                    <div class="mugshot">
                        <div class="mugshot-placeholder">
                            <span class="icon">❓</span>
                            UR CALL<br>HERE??
                        </div>
                    </div>
                    <div class="member-info">
                        <div class="member-callsign">??????</div>
                        <div class="member-number">{number}</div>
                        <div class="member-name">Could be U!!</div>
                    </div>
                </div>"""


def roster_layout() -> str:
    """The ROSTER_LAYOUT environment value, or the default when unset or blank."""
    layout = os.environ.get("ROSTER_LAYOUT", "").strip() or DEFAULT_ROSTER_LAYOUT
    if layout not in ROSTER_LAYOUTS:
        raise ValueError(f"Unknown ROSTER_LAYOUT {layout!r}; use one of: {', '.join(ROSTER_LAYOUTS)}")
    return layout


def render_roster_block(members: list[dict], layout: str = DEFAULT_ROSTER_LAYOUT) -> str:
    if layout == "list":
        return render_roster_list(members)
    return render_roster_classic(members)


def render_roster_classic(members: list[dict]) -> str:
    if not members:
        return ""
    new_numbers = {m["number"] for m in members[-NEW_BADGE_LIMIT:]}
    cards: list[str] = []
    for member in members:
        if member["number"] == 1:
            cards.append(render_founder_card(member))
        else:
            cards.append(
                render_member_card(
                    member,
                    is_new=member["number"] in new_numbers,
                )
            )
    next_number = max(m["number"] for m in members) + 1
    cards.append(render_ghost_card(next_number))
    return "\n\n".join(cards)


def roster_place(member: dict) -> tuple[str, str]:
    """(territory, flag) for the list layout: ("UT", "") or ("Canada", "🇨🇦")."""
    bucket = member_map_bucket(member)
    if bucket is None:
        return "", ""
    kind, where = bucket
    return where, (COUNTRY_FLAGS.get(where.lower(), "🌍") if kind == "dx" else "")


def roster_initials(name: str) -> str:
    letters = [word[0] for word in name.split() if word[:1].isalpha()]
    return "".join(letters[:2]).upper() or "?"


def roster_thumb_html(member: dict) -> str:
    path = member.get("mugshot_path")
    if path:
        callsign = html_escape(member["callsign"])
        return f'<img src="{html_escape(path)}" alt="{callsign} profile photo" loading="lazy">'
    return f'<span class="rl-ini">{html_escape(roster_initials(member["name"]))}</span>'


def render_roster_row(member: dict, *, is_new: bool) -> str:
    callsign = html_escape(member["callsign"])
    name = html_escape(member["name"])
    place, flag = roster_place(member)
    sponsor = member.get("sponsor_member")
    badges = ""
    if member["number"] == 1:
        badges += '<span class="rl-b rl-boss">GODFATHER</span>'
    badges += territory_og_badge_html(member).strip()
    if is_new:
        badges += '<span class="rl-b rl-new">NEW</span>'
    data = (
        f' data-n="{member["number"]}"'
        f' data-joined="{parse_join_date(member.get("join_date", "")) or ""}"'
        f' data-place="{html_escape(place)}" data-flag="{flag}"'
        f' data-sponsor="{html_escape(sponsor["callsign"]) if sponsor else ""}"'
    )
    return f"""                    <li class="rl-row" id="{callsign}"{data}>
                        <span class="rl-shot">{roster_thumb_html(member)}</span>
                        <div class="rl-main">
                            <div class="rl-line"><a href="https://www.qrz.com/db/{callsign}" target="_blank" rel="noopener" class="member-callsign">{callsign}</a><div class="rl-badges">{badges}</div></div>
                            <div class="rl-who"><div class="member-name">{name}</div><div class="rl-place">{flag + " " if flag else ""}{html_escape(place)}</div></div>
                        </div>
                        <div class="member-number">BKG #{member["number"]:03d}</div>
                    </li>"""


def render_roster_ghost_row(next_number: int) -> str:
    return f"""                    <li class="rl-row rl-ghost">
                        <span class="rl-shot"><span class="rl-ini">?</span></span>
                        <div class="rl-main">
                            <div class="rl-line"><a href="#join" class="rl-ghost-call">UR CALL HERE??</a></div>
                            <div class="rl-who"><div class="member-name">Could be U!!</div></div>
                        </div>
                        <div class="member-number">BKG #{next_number:03d}</div>
                    </li>"""


def render_fame_card(member: dict, tag: str, tone: str, sub: str) -> str:
    callsign = html_escape(member["callsign"])
    return (
        f'<a class="rl-fcard rl-{tone}" href="#{callsign}" data-call="{callsign}">'
        f'<span class="rl-tag">{tag}</span><span class="rl-shot">{roster_thumb_html(member)}</span>'
        f'<span class="rl-fcall">{callsign}</span><span class="rl-sub">{sub}</span></a>'
    )


def render_roster_list(members: list[dict]) -> str:
    """Searchable roster: list or mugshot wall, blocks of 100, rap sheets.

    Every member is server-rendered (so the page works without JavaScript and
    site_contract.py still sees each callsign, name and number); assets/roster.js
    adds search, filters, sorting, the wall view and the rap sheet on top. Photos
    and places come only from the member data already prepared for this build.
    """
    if not members:
        return ""
    new_numbers = {m["number"] for m in members[-NEW_BADGE_LIMIT:]}
    recruits: dict[int, int] = {}
    for m in members:
        if m.get("sponsor_member"):
            sponsor_number = m["sponsor_member"]["number"]
            recruits[sponsor_number] = recruits.get(sponsor_number, 0) + 1

    def count(n: int) -> str:
        return f"{n} recruit" + ("" if n == 1 else "s")

    fame = []
    founder = next((m for m in members if m["number"] == 1), None)
    if founder:
        fame.append(render_fame_card(founder, "👑 Godfather", "gold", f"BKG #001 · {count(recruits.get(1, 0))}"))
    top = sorted((m for m in members if m["number"] != 1 and recruits.get(m["number"])),
                 key=lambda m: (-recruits[m["number"]], m["number"]))[:FAME_RECRUITERS]
    for rank, m in enumerate(top, 1):
        fame.append(render_fame_card(m, f"Top recruiter #{rank}", "red",
                                     f"BKG #{m['number']:03d} · {count(recruits[m['number']])}"))
    for m in reversed(members[-FAME_NEWEST:]):
        fame.append(render_fame_card(m, "Fresh meat", "lime", f"BKG #{m['number']:03d}"))

    blocks = []
    next_number = max(m["number"] for m in members) + 1
    by_block: dict[int, list[dict]] = {}
    for m in members:
        by_block.setdefault((m["number"] - 1) // ROSTER_BLOCK_SIZE, []).append(m)
    last_block = max(by_block)
    for index in sorted(by_block):
        group = by_block[index]
        low = index * ROSTER_BLOCK_SIZE + 1
        rows = [
            render_roster_row(m, is_new=m["number"] in new_numbers)
            for m in group
        ]
        if index == last_block:
            rows.append(render_roster_ghost_row(next_number))
        blocks.append(
            f"""                <section class="rl-block" id="rl-{low:03d}">
                    <div class="rl-head"><h3>#{low:03d}–#{low + ROSTER_BLOCK_SIZE - 1:03d}</h3><span>{len(group)} members</span></div>
                    <ol class="rl-rows">
{chr(10).join(rows)}
                    </ol>
                </section>"""
        )
    jump = "".join(f'<a href="#rl-{i * ROSTER_BLOCK_SIZE + 1:03d}">#{i * ROSTER_BLOCK_SIZE + 1:03d}</a>' for i in sorted(by_block))
    return f"""                <div class="rl" data-view="list">
                <div class="rl-bar" hidden>
                    <div class="rl-search">
                        <input id="rl-q" type="search" placeholder="Call, name, # or state" autocomplete="off" autocapitalize="characters" spellcheck="false" aria-label="Search the roster">
                        <div class="rl-views" role="group" aria-label="View">
                            <button type="button" data-view="list" aria-pressed="true" title="List"><svg viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><rect x="2" y="3" width="16" height="3"/><rect x="2" y="8.5" width="16" height="3"/><rect x="2" y="14" width="16" height="3"/></svg><span class="rl-sr">List</span></button>
                            <button type="button" data-view="wall" aria-pressed="false" title="Mugshot wall"><svg viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><rect x="2" y="2" width="7" height="7"/><rect x="11" y="2" width="7" height="7"/><rect x="2" y="11" width="7" height="7"/><rect x="11" y="11" width="7" height="7"/></svg><span class="rl-sr">Mugshot wall</span></button>
                        </div>
                    </div>
                    <div class="rl-chips" role="group" aria-label="Filter and sort">
                        <button type="button" class="rl-chip" data-filter="all" aria-pressed="true">Everybody</button>
                        <button type="button" class="rl-chip" data-filter="new" aria-pressed="false">Fresh meat · 30 days</button>
                        <button type="button" class="rl-chip" data-filter="og" aria-pressed="false">OGs</button>
                        <button type="button" class="rl-chip" data-filter="recruiters" aria-pressed="false">Recruiters</button>
                        <button type="button" class="rl-chip" data-filter="photo" aria-pressed="false">Has a mugshot</button>
                        <button type="button" class="rl-chip rl-sort" aria-pressed="false">Sort: BKG #</button>
                    </div>
                    <div class="rl-result" aria-live="polite"></div>
                </div>
                <section class="rl-fame" aria-label="Most wanted">
                    <h3>MOST WANTED</h3>
                    <div class="rl-rail">{"".join(fame)}</div>
                </section>
                <nav class="rl-jump" aria-label="Jump to block">{jump}</nav>
                <div class="rl-out">
{chr(10).join(blocks)}
                </div>
                </div>"""


def render_downline_data(members: list[dict]) -> str:
    """Build the JSON downline data: flat list of {call, name, num, sponsor}.

    'sponsor' is the sponsoring member's callsign or null (a root). The tree
    in tree.html (#bkg-downline-data) assembles the forest client-side.
    """
    nodes = [
        {
            "call": m["callsign"],
            "name": m["name"],
            "num": m["number"],
            "sponsor": m["sponsor_member"]["callsign"] if m.get("sponsor_member") else None,
        }
        for m in sorted(members, key=lambda m: m["number"])
    ]
    return embedded_json(nodes)


def render_map_data(members: list[dict]) -> str:
    """Build the JSON map data consumed by the territory map (#bkg-map-data):

    {"states": {state_abbr: [{"call", "name", "num"}, ...]},
     "dx": {country: {"flag": "🇨🇦", "members": [{"call", "name", "num"}, ...]}}}

    Reviewed QTH places US members in "states" and international members in
    "dx", which renders in the map's inset box.
    """
    by_state: dict[str, list[dict]] = {}
    by_country: dict[str, list[dict]] = {}
    for member in members:
        bucket = member_map_bucket(member)
        if not bucket:
            continue
        entry = {
            "call": member["callsign"],
            "name": member["name"],
            "num": member["number"],
        }
        kind, key = bucket
        target = by_state if kind == "state" else by_country
        target.setdefault(key, []).append(entry)
    for entries in (*by_state.values(), *by_country.values()):
        entries.sort(key=lambda e: e["num"])
    data = {
        "states": {state: by_state[state] for state in sorted(by_state)},
        "dx": {
            country: {
                "flag": COUNTRY_FLAGS.get(country.lower(), "🌍"),
                "members": by_country[country],
            }
            for country in sorted(by_country)
        },
    }
    return embedded_json(data)


def member_latlon(member: dict) -> tuple[float, float] | None:
    """Return the published coordinates exactly as supplied by administration."""
    location = member["map_location"]
    return (location["lat"], location["lon"]) if location is not None else None


def render_nearby_data(members: list[dict]) -> tuple[str, str]:
    """Build the coordinates and unmapped member identities for nearby.html.

    Map entries include call, name, number, and the published grid and coordinates.
    Members without a usable location contribute only a callsign and member
    number to the local search index.
    """
    entries = []
    unmapped = []
    for member in sorted(members, key=lambda m: m["number"]):
        grid = member.get("grid")
        latlon = member_latlon(member)
        if latlon is None:
            unmapped.append({"call": member["callsign"], "num": member["number"]})
            continue
        entry = {
            "call": member["callsign"],
            "name": member["name"],
            "num": member["number"],
            "lat": latlon[0],
            "lon": latlon[1],
        }
        if grid:
            entry["grid"] = grid
        entries.append(entry)
    print(f"  Geo coords for {len(entries)}/{len(members)} members", file=sys.stderr)
    return embedded_json(entries), embedded_json(unmapped)


def update_nearby(members: list[dict], output_dir: Path) -> None:
    """Inject both roster datasets into the Nearby page."""
    html = NEARBY_PATH.read_text(encoding="utf-8")
    geo, unmapped = render_nearby_data(members)
    html = replace_between(html, "<!-- GEO_DATA:START -->", "<!-- GEO_DATA:END -->", geo)
    html = replace_between(html, "<!-- UNMAPPED_MEMBERS:START -->", "<!-- UNMAPPED_MEMBERS:END -->", unmapped)
    (output_dir / "nearby.html").write_text(html, encoding="utf-8")


# Join Date cell formats seen (or plausible) on the roster sheet. Google
# Reviewed QSO dates retain the formats used when they were recorded.
JOIN_DATE_FORMATS = (
    "%Y-%m-%d", "%Y/%m/%d", "%m/%d/%Y", "%m/%d/%y", "%m-%d-%Y", "%m-%d-%y",
    "%b %d, %Y", "%B %d, %Y", "%b %d %Y", "%B %d %Y", "%d %b %Y", "%d %B %Y",
    "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%m/%d/%Y %H:%M:%S",
)


def join_date_iso(value: datetime) -> str:
    # Treat accidental leading zeros (0026) like our supported two-digit years
    # (26), using the same %y century rule instead of a historical year.
    if value.year < 100:
        year = datetime.strptime(f"{value.year:02d}", "%y").year
        value = value.replace(year=year)
    # strftime's %Y padding differs on macOS and Linux for years <1000.
    return value.date().isoformat()


def parse_join_date(raw: str) -> str | None:
    """Normalize a roster 'Join Date' cell to 'YYYY-MM-DD', or None if unreadable."""
    text = (raw or "").strip()
    if not text:
        return None
    for fmt in JOIN_DATE_FORMATS:
        try:
            return join_date_iso(datetime.strptime(text, fmt))
        except ValueError:
            continue
    # Last resort: a YYYY-MM-DD prefix on something longer (e.g. an ISO timestamp with tz)
    match = re.match(r"(\d{4})-(\d{2})-(\d{2})", text)
    if match:
        try:
            return join_date_iso(datetime(int(match[1]), int(match[2]), int(match[3])))
        except ValueError:
            return None
    return None


def render_outbreak_data(members: list[dict]) -> str:
    """Build the JSON outbreak data consumed by outbreak.html (#bkg-outbreak-data):

    [{"call", "name", "num", "date", "state", "dx", "flag", "sponsor", "lat", "lon"}, ...]
    ordered by member number. "date" is the normalized join date or null
    (the page interpolates missing dates from neighbouring member numbers),
    "state" is the two-letter US state for members on the map, "dx" the
    country for international members (with "flag"), "sponsor" the
    recruiting member's callsign or null — the transmission chain — and
    "lat"/"lon" the same published coordinates nearby.html gets (present only
    when administration supplies a map location; the page falls back to the state).
    """
    entries = []
    dated = 0
    located = 0
    for member in sorted(members, key=lambda m: m["number"]):
        date = parse_join_date(member.get("join_date", ""))
        if date:
            dated += 1
        elif member.get("join_date"):
            print(
                f"  Unreadable join date for {member['callsign']}: {member['join_date']!r}",
                file=sys.stderr,
            )
        entry = {
            "call": member["callsign"],
            "name": member["name"],
            "num": member["number"],
            "date": date,
            "state": None,
            "dx": None,
            "sponsor": member["sponsor_member"]["callsign"] if member.get("sponsor_member") else None,
        }
        bucket = member_map_bucket(member)
        if bucket and bucket[0] == "state":
            entry["state"] = bucket[1]
        elif bucket:
            entry["dx"] = bucket[1]
            entry["flag"] = COUNTRY_FLAGS.get(bucket[1].lower(), "🌍")
        latlon = member_latlon(member)
        if latlon is not None:
            entry["lat"], entry["lon"] = latlon
            located += 1
        entries.append(entry)
    print(f"  Join dates parsed for {dated}/{len(entries)} members", file=sys.stderr)
    print(f"  Outbreak coords for {located}/{len(entries)} members", file=sys.stderr)
    return embedded_json(entries)


def update_outbreak(members: list[dict], output_dir: Path) -> None:
    """Inject the outbreak JSON from the source template into the public build."""
    html = OUTBREAK_PATH.read_text(encoding="utf-8")
    html = replace_between(
        html,
        "<!-- OUTBREAK_DATA:START -->",
        "<!-- OUTBREAK_DATA:END -->",
        render_outbreak_data(members),
    )
    (output_dir / "outbreak.html").write_text(html, encoding="utf-8")


US_STATE_NAMES = {
    "AL": "Alabama", "AK": "Alaska", "AZ": "Arizona", "AR": "Arkansas",
    "CA": "California", "CO": "Colorado", "CT": "Connecticut", "DE": "Delaware",
    "DC": "District of Columbia", "FL": "Florida", "GA": "Georgia", "HI": "Hawaii",
    "ID": "Idaho", "IL": "Illinois", "IN": "Indiana", "IA": "Iowa", "KS": "Kansas",
    "KY": "Kentucky", "LA": "Louisiana", "ME": "Maine", "MD": "Maryland",
    "MA": "Massachusetts", "MI": "Michigan", "MN": "Minnesota", "MS": "Mississippi",
    "MO": "Missouri", "MT": "Montana", "NE": "Nebraska", "NV": "Nevada",
    "NH": "New Hampshire", "NJ": "New Jersey", "NM": "New Mexico", "NY": "New York",
    "NC": "North Carolina", "ND": "North Dakota", "OH": "Ohio", "OK": "Oklahoma",
    "OR": "Oregon", "PA": "Pennsylvania", "RI": "Rhode Island", "SC": "South Carolina",
    "SD": "South Dakota", "TN": "Tennessee", "TX": "Texas", "UT": "Utah",
    "VT": "Vermont", "VA": "Virginia", "WA": "Washington", "WV": "West Virginia",
    "WI": "Wisconsin", "WY": "Wyoming",
}
STATE_CODES_BY_NAME = {name.lower(): code for code, name in US_STATE_NAMES.items()}
STATE_CODES_BY_NAME.update({"washington dc": "DC", "washington, dc": "DC", "washington d.c.": "DC", "washington, d.c.": "DC"})

# Canadian provinces and territories, by postal code.
CANADIAN_PROVINCE_NAMES = {
    "AB": "Alberta", "BC": "British Columbia", "MB": "Manitoba", "NB": "New Brunswick",
    "NL": "Newfoundland and Labrador", "NS": "Nova Scotia", "NT": "Northwest Territories",
    "NU": "Nunavut", "ON": "Ontario", "PE": "Prince Edward Island", "QC": "Quebec",
    "SK": "Saskatchewan", "YT": "Yukon",
}
PROVINCE_CODES_BY_NAME = {name.lower(): code for code, name in CANADIAN_PROVINCE_NAMES.items()}
PROVINCE_CODES_BY_NAME.update({
    "newfoundland": "NL", "newfoundland & labrador": "NL", "québec": "QC",
    "pei": "PE", "yukon territory": "YT",
})


def embedded_json(data) -> str:
    """Escape HTML-sensitive characters while preserving JSON field values."""
    return (json.dumps(data, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
            .replace("&", "\\u0026").replace("<", "\\u003c").replace(">", "\\u003e")
            .replace("\u2028", "\\u2028").replace("\u2029", "\\u2029"))


def replace_between(html: str, start_marker: str, end_marker: str, replacement: str) -> str:
    pattern = re.compile(
        re.escape(start_marker) + r".*?" + re.escape(end_marker),
        re.DOTALL,
    )
    if not pattern.search(html):
        raise RuntimeError(f"Markers not found: {start_marker} ... {end_marker}")
    # Use a function replacement so backslashes in `replacement` (e.g. \uXXXX
    # in the JSON map data) are inserted literally instead of being treated as
    # regex backreferences/escapes.
    new_block = start_marker + replacement + end_marker
    return pattern.sub(lambda _match: new_block, html, count=1)


def update_index(members: list[dict], output_dir: Path) -> None:
    html = INDEX_PATH.read_text(encoding="utf-8")
    layout = roster_layout()
    roster = render_roster_block(members, layout)
    html = replace_between(
        html,
        ROSTER_START,
        "<!-- ROSTER:END -->",
        "\n" + roster + "\n                ",
    )
    html = replace_between(
        html,
        "<!-- MEMBER_COUNT:START -->",
        "<!-- MEMBER_COUNT:END -->",
        str(len(members)),
    )
    html = replace_between(
        html,
        "<!-- BUILD_TIME:START -->",
        "<!-- BUILD_TIME:END -->",
        datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC"),
    )
    html = replace_between(
        html,
        "<!-- MAP_DATA:START -->",
        "<!-- MAP_DATA:END -->",
        render_map_data(members),
    )
    (output_dir / "index.html").write_text(html, encoding="utf-8")


def update_tree(members: list[dict], output_dir: Path) -> None:
    """Inject the downline JSON from the source template into the public build."""
    html = TREE_PATH.read_text(encoding="utf-8")
    html = replace_between(
        html,
        "<!-- DOWNLINE_DATA:START -->",
        "<!-- DOWNLINE_DATA:END -->",
        render_downline_data(members),
    )
    (output_dir / "tree.html").write_text(html, encoding="utf-8")


def render_members_txt(members: list[dict]) -> str:
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
    lines = [
        "# BKG Callsign Notes for Ham2K PoLo",
        f"# Generated: {now}",
        "# Do not edit manually - this file is auto-generated",
        "",
    ]
    for member in sorted(members, key=lambda m: m["callsign"]):
        line = f"{member['callsign']} 🤜 {member['name']} BKG #{member['number']}"
        tags = [f"{region['region_label']} OG" for region in member["og_regions"]]
        if tags:
            line += f" ({', '.join(tags)})"
        lines.append(line)
    lines.append("")
    return "\n".join(lines)


def stage_public_files(output_dir: Path) -> None:
    """Copy the static site files and bundled arcade game."""
    for relative in PUBLIC_FILES:
        source = REPO_ROOT / relative
        destination = output_dir / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
    game_root = REPO_ROOT / GAME_DIR
    for source in sorted(game_root.rglob("*")):
        if source.is_symlink():
            raise ValueError(f"Symlink in game directory: {source}")
        if source.is_file():
            destination = output_dir / GAME_DIR / source.relative_to(game_root)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)


def build(output_dir: Path, *, roster_json: Path | None = None,
          asset_dir: Path | None = None, asset_origin: str | None = None) -> None:
    exclusions = []
    envelope, members = parse_roster_json(
        read_roster_json(roster_json), exclude_invalid_callsigns=True, exclusions=exclusions
    )
    for exclusion in exclusions:
        print(f"WARNING: Excluding BKG #{exclusion['bkg_number']}: {exclusion['reason']}", file=sys.stderr)
    for member in members:
        member["state"], member["country"] = qth_location(member["qth"])
    canonicalize_countries(members)
    photos = annotate_public_assets(members, asset_dir, source_url=asset_origin, repo_root=REPO_ROOT)
    stage_public_files(output_dir)
    for relative, photo in photos.items():
        destination = output_dir / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(photo, destination)
    snapshot = output_dir / PUBLIC_ROSTER_PATH
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    snapshot.write_text(json.dumps(envelope, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Validated JSON roster: {len(members)} members ({envelope['content_hash']})")

    update_index(members, output_dir)
    print(f"Updated {INDEX_PATH.name}")

    update_tree(members, output_dir)
    print(f"Updated {TREE_PATH.name}")

    update_nearby(members, output_dir)
    print(f"Updated {NEARBY_PATH.name}")

    update_outbreak(members, output_dir)
    print(f"Updated {OUTBREAK_PATH.name}")

    (output_dir / "members.txt").write_text(render_members_txt(members), encoding="utf-8")
    print("Wrote members.txt")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output-dir", type=Path, default=REPO_ROOT / "dist",
        help="public build directory (default: dist/)",
    )
    parser.add_argument("--roster-json", type=Path,
                        help="saved V2 roster export; otherwise fetch from ROSTER_EXPORT_URL")
    parser.add_argument("--asset-dir", type=Path,
                        help="immutable administration photo bytes for a v2 snapshot, outside the source tree")
    parser.add_argument("--asset-origin", default=os.environ.get("ROSTER_EXPORT_URL"),
                        help="trusted HTTPS roster URL for v2 photo origin verification (or ROSTER_EXPORT_URL)")
    args = parser.parse_args(argv)
    output_dir = args.output_dir.resolve()
    try:
        source_root = REPO_ROOT.resolve()
        if (output_dir == source_root or output_dir in source_root.parents
                or (output_dir.is_relative_to(source_root) and output_dir != source_root / "dist")):
            raise ValueError("Output must be dist/ or a directory outside the source repository")
        if args.asset_dir:
            assets = args.asset_dir.resolve()
            if (assets.is_relative_to(source_root) or source_root.is_relative_to(assets)
                    or assets.is_relative_to(output_dir) or output_dir.is_relative_to(assets)):
                raise ValueError("Public assets must be outside the repository and separate from output")
        if args.roster_json and args.roster_json.resolve().is_relative_to(output_dir):
            raise ValueError("JSON input must be outside its output directory")
        # Never let a failed generation leave a half-built output directory.
        output_dir.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(
            prefix=f".{output_dir.name}-", dir=output_dir.parent
        ) as temporary:
            staged = Path(temporary) / "public"
            staged.mkdir()
            build(staged, roster_json=args.roster_json, asset_dir=args.asset_dir, asset_origin=args.asset_origin)
            validate_dist(staged)
            previous = Path(temporary) / "previous"
            if output_dir.exists():
                if not output_dir.is_dir():
                    raise ValueError("Output must be a directory")
                output_dir.rename(previous)
            try:
                staged.rename(output_dir)
            except Exception:
                if previous.exists():
                    previous.rename(output_dir)
                raise
    except Exception as exc:
        print(f"ERROR: Build failed: {exc}", file=sys.stderr)
        return 1
    print(f"Built public site in {output_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
