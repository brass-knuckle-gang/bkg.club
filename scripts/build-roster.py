#!/usr/bin/env python3
"""Fetch the BKG roster CSV from Google Sheets and build the public site in dist/.

The roster section of index.html is rebuilt between <!-- ROSTER:START --> and
<!-- ROSTER:END --> markers. The members count is updated between
<!-- MEMBER_COUNT:START --> and <!-- MEMBER_COUNT:END -->. members.txt is a
Ham2K PoLo callsign notes file (auto-generated, not manually edited).

tree.html, nearby.html and outbreak.html each get a JSON payload injected
between their own START/END markers (downline, geo, and outbreak data).

QRZ lookups are cached in qrz-cache.json (see QRZ_CACHE_* below); the deploy
workflow commits the refreshed cache back to main after each build.
"""

import argparse
import csv
import http.client
import io
import json
import os
import re
import shutil
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

from site_contract import PHOTO_DIRS, PUBLIC_FILES

SHEET_ID = "1GPNjke3fDf18amh3KbUpUAJUqMu4nOuFLm1F8CDzbrY"
SHEET_CSV_URL = f"https://docs.google.com/spreadsheets/d/{SHEET_ID}/export?format=csv"

REPO_ROOT = Path(__file__).resolve().parent.parent
INDEX_PATH = REPO_ROOT / "index.html"
TREE_PATH = REPO_ROOT / "tree.html"
NEARBY_PATH = REPO_ROOT / "nearby.html"
OUTBREAK_PATH = REPO_ROOT / "outbreak.html"
NAME_OVERRIDES_PATH = REPO_ROOT / "name-overrides.txt"
LOCATION_OVERRIDES_PATH = REPO_ROOT / "location-overrides.txt"
MUGSHOT_DIR = REPO_ROOT / "images" / "mugshots"
MUGSHOT_REL_DIR = "images/mugshots"
MUGSHOT_OVERRIDE_DIR = REPO_ROOT / "images" / "mugshots-override"
MUGSHOT_EXTS = {".jpg", ".jpeg", ".png", ".gif", ".webp"}
# Callsign -> QRZ image URL of each file
MUGSHOT_SOURCES_PATH = MUGSHOT_DIR / ".sources.json"

# QRZ lookup cache (qrz-cache.json, committed back by the deploy workflow).
# A full roster is one QRZ request per member; almost all of them repeat the
# Every build looks up every member live. The
# cache holds only changed callsigns, as the
# fallback when lookups fail.
QRZ_CACHE_PATH = REPO_ROOT / "qrz-cache.json"
QRZ_MAX_CONSECUTIVE_FAILURES = 5  # then use cache only
QRZ_CACHE_ABOUT = (
    "Current callsign per sheet callsign, from QRZ. Used only when a QRZ "
    "lookup fails. Auto-refreshed by scripts/build-roster.py; safe to delete."
)

NEW_BADGE_LIMIT = 3  # last N members get the "NEW!!" badge
OG_BADGE_NUMBERS = {2}  # member numbers that get the "OG" badge (founder #1 has its own treatment)

QRZ_XML_URL = "https://xmldata.qrz.com/xml/current/"
QRZ_NS = {"q": "http://xmldata.qrz.com"}

# The 50 states + DC drawn in the index.html SVG map.
US_MAP_STATES = frozenset(
    """
    AL AK AZ AR CA CO CT DE DC FL GA HI ID IL IN IA KS KY LA ME MD MA MI MN
    MS MO MT NE NV NH NJ NM NY NC ND OH OK OR PA RI SC SD TN TX UT VT VA WA
    WV WI WY
    """.split()
)

# QRZ country values (DXCC-style entity names) that belong on the US map
# rather than in the international DX box. Compared lowercase.
US_DXCC_NAMES = {"united states", "united states of america", "usa", "alaska", "hawaii"}

# DXCC entity name (as QRZ reports it, lowercased) -> flag emoji for the DX box.
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
    international members, or None when there's nothing to plot. QRZ reports
    Canadian provinces and other subdivisions in the same <state> field as US
    states, so a bare two-letter code is only trusted when the country agrees.
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


def fetch_csv() -> str:
    req = urllib.request.Request(
        SHEET_CSV_URL,
        headers={"User-Agent": "BKG-Roster-Builder/1.0"},
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return resp.read().decode("utf-8")


def parse_member_number(raw: str) -> int:
    """Extract integer from values like 'BKG1', 'BKG #1', '1'."""
    match = re.search(r"\d+", raw or "")
    return int(match.group(0)) if match else 0


def _norm_header(name: str) -> str:
    """Lowercase a header and collapse punctuation/whitespace to single spaces."""
    return re.sub(r"[^a-z0-9]+", " ", (name or "").lower()).strip()


# Header names (normalized) that mean "who recruited this member". The roster
# sheet's column is expected to be "Sponsor"; these aliases keep the build
# working if it's titled differently.
SPONSOR_HEADER_ALIASES = {
    "sponsor", "sponsored by", "sponsor callsign", "sponsor call",
    "recruiter", "recruited by", "referred by", "brought in by",
    "elmer", "upline",
}


def find_sponsor_header(fieldnames: list[str] | None) -> str | None:
    """Pick the sponsor column from the CSV headers, tolerant of naming/case.

    Matches a known alias first, then any header mentioning sponsor/recruit/
    upline/elmer. Returns the original header string, or None if absent.
    """
    headers = [h for h in (fieldnames or []) if h]
    norm_to_orig = {_norm_header(h): h for h in headers}
    for alias in SPONSOR_HEADER_ALIASES:
        if alias in norm_to_orig:
            return norm_to_orig[alias]
    for h in headers:
        n = _norm_header(h)
        if any(kw in n for kw in ("sponsor", "recruit", "upline", "elmer")):
            return h
    return None


def parse_members(csv_text: str) -> list[dict]:
    reader = csv.DictReader(io.StringIO(csv_text))
    fieldnames = reader.fieldnames
    sponsor_key = find_sponsor_header(fieldnames)
    # Surface the sheet schema so a missing/renamed sponsor column (or the CSV
    # export hitting the wrong tab) is obvious in the build log.
    print(f"  CSV columns: {fieldnames}", file=sys.stderr)
    print(f"  Sponsor column detected as: {sponsor_key!r}", file=sys.stderr)
    if sponsor_key is None:
        print(
            "  WARNING: no sponsor column found — the downline tree will be flat. "
            "Expected a 'Sponsor' column on the exported sheet tab.",
            file=sys.stderr,
        )
    members = []
    for row in reader:
        callsign = (row.get("Callsign") or "").strip()
        name = (row.get("Name") or "").strip()
        join_date = (row.get("Join Date") or "").strip()
        qth = (row.get("QTH") or "").strip()
        number = parse_member_number(row.get("#") or row.get("BKG #") or "")
        # "Sponsor" = who recruited this member, as a callsign or BKG number.
        # Resolved to the sponsoring member in resolve_sponsors().
        sponsor_raw = ((row.get(sponsor_key) if sponsor_key else "") or "").strip()
        if callsign and number:
            members.append(
                {
                    "callsign": callsign,
                    "name": name,
                    "join_date": join_date,
                    "qth": qth,
                    "number": number,
                    "sponsor_raw": sponsor_raw,
                    "sponsor_member": None,
                }
            )
    members.sort(key=lambda m: m["number"])
    return members


def validate_members(members: list[dict]) -> None:
    """Reject unusable rosters before they can replace the deployed site.

    Check again after QRZ canonicalizes callsigns: two sheet rows mapping to
    the same operator would otherwise overwrite each other in page lookups.
    """
    if not members:
        raise ValueError("No members parsed from CSV")
    numbers: set[int] = set()
    callsigns: set[str] = set()
    for member in members:
        number = member["number"]
        callsign = member["callsign"].upper()
        if number in numbers:
            raise ValueError(f"Duplicate member number: {number}")
        if callsign in callsigns:
            raise ValueError(f"Duplicate callsign: {callsign}")
        numbers.add(number)
        callsigns.add(callsign)


def load_name_overrides() -> dict[str, str]:
    """Load callsign -> display-name overrides from name-overrides.txt.

    This file is manually maintained (unlike members.txt) and lets anyone
    override the name a member is shown with — the roster sheet stays the
    source of truth, but the override wins. It's the text-name analogue of the
    images/mugshots-override/ directory. Format, one entry per line:

        CALLSIGN = Display Name

    Blank lines and lines starting with '#' are ignored. The separator may be
    '=' or ':'. Callsigns are matched case-insensitively. Returns a dict keyed
    by upper-cased callsign; empty if the file is absent.
    """
    if not NAME_OVERRIDES_PATH.is_file():
        return {}
    overrides: dict[str, str] = {}
    for lineno, raw_line in enumerate(NAME_OVERRIDES_PATH.read_text().splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        match = re.match(r"([A-Za-z0-9/]+)\s*[=:]\s*(.+)$", line)
        if not match:
            print(
                f"  Skipping unparseable name override ({NAME_OVERRIDES_PATH.name}:{lineno}): {raw_line!r}",
                file=sys.stderr,
            )
            continue
        callsign = match.group(1).strip().upper()
        name = match.group(2).strip()
        if name:
            overrides[callsign] = name
    return overrides


def apply_name_overrides(members: list[dict], overrides: dict[str, str]) -> None:
    """Replace member['name'] with a name-overrides.txt entry, in place.

    Matches on the roster callsign (case-insensitive). Applied before the QRZ
    lookup so it needs no network/credentials. Logs each applied override and
    any override whose callsign isn't (yet) in the roster.
    """
    if not overrides:
        return
    used: set[str] = set()
    for member in members:
        key = member["callsign"].upper()
        new_name = overrides.get(key)
        if new_name and new_name != member["name"]:
            print(
                f"  Name override: {member['callsign']} {member['name']!r} -> {new_name!r}",
                file=sys.stderr,
            )
            member["name"] = new_name
        if new_name:
            # Overrides are shown verbatim in members.txt
            member["name_override"] = True
            used.add(key)
    for key in overrides.keys() - used:
        print(
            f"  Name override for {key} not applied (callsign not in roster)",
            file=sys.stderr,
        )


def load_location_overrides() -> dict[str, tuple[str | None, str | None]]:
    """Load callsign -> (state, country) overrides from location-overrides.txt.

    Manually maintained, like name-overrides.txt. Overrides the QRZ-reported
    location for members whose QRZ QTH is stale — this moves them on the
    territory map, which also shifts state/country OG badges, since the OG is
    simply the lowest-numbered member in each territory. Format, one entry per
    line:

        CALLSIGN = UT              (two-letter US state code)
        CALLSIGN = Canada          (anything else: DXCC country name)

    Blank lines and lines starting with '#' are ignored. The separator may be
    '=' or ':'. Callsigns are matched case-insensitively. Returns a dict keyed
    by upper-cased callsign mapping to (state, country) — a US-state entry
    yields ("UT", "United States"), a country entry yields (None, name).
    """
    if not LOCATION_OVERRIDES_PATH.is_file():
        return {}
    overrides: dict[str, tuple[str | None, str | None]] = {}
    for lineno, raw_line in enumerate(LOCATION_OVERRIDES_PATH.read_text().splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        match = re.match(r"([A-Za-z0-9/]+)\s*[=:]\s*(.+)$", line)
        if not match:
            print(
                f"  Skipping unparseable location override ({LOCATION_OVERRIDES_PATH.name}:{lineno}): {raw_line!r}",
                file=sys.stderr,
            )
            continue
        callsign = match.group(1).strip().upper()
        value = match.group(2).strip()
        if not value:
            continue
        if value.upper() in US_MAP_STATES:
            overrides[callsign] = (value.upper(), "United States")
        else:
            overrides[callsign] = (None, value)
    return overrides


def apply_location_overrides(
    members: list[dict], overrides: dict[str, tuple[str | None, str | None]]
) -> None:
    """Replace member['state']/member['country'] with location-overrides.txt
    entries, in place.

    Matches on the (QRZ-canonicalized) callsign, case-insensitively. Must run
    after the QRZ lookups and before mark_territory_ogs() so the OG badges are
    computed from the overridden locations. Logs each applied override and any
    override whose callsign isn't (yet) in the roster.
    """
    if not overrides:
        return
    used: set[str] = set()
    for member in members:
        key = member["callsign"].upper()
        override = overrides.get(key)
        if override is None:
            continue
        used.add(key)
        state, country = override
        if (state, country) != (member.get("state"), member.get("country")):
            print(
                f"  Location override: {member['callsign']} "
                f"{member.get('state')!r}/{member.get('country')!r} -> {state!r}/{country!r}",
                file=sys.stderr,
            )
            member["state"] = state
            member["country"] = country
            # overridden = QRZ QTH is stale, so its coords are too
            member["grid"] = None
            member["lat"] = None
            member["lon"] = None
    for key in overrides.keys() - used:
        print(
            f"  Location override for {key} not applied (callsign not in roster)",
            file=sys.stderr,
        )


# A full build is one QRZ request per member (500+), and QRZ drops or times
# out a handful of connections per day. Retry each request a few times with
# a short backoff before giving up on it, so one hiccup neither kills the
# deploy nor silently loses a member's location for that build.
FETCH_ATTEMPTS = 3
FETCH_BACKOFF_SECONDS = (2, 5)


def _fetch_with_retry(req: urllib.request.Request, *, timeout: float, what: str) -> bytes | None:
    """GET a URL, retrying transient network failures. Returns None if all attempts fail."""
    for attempt in range(1, FETCH_ATTEMPTS + 1):
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return resp.read()
        except (OSError, http.client.HTTPException) as exc:
            # OSError covers urllib.error.URLError, socket timeouts and
            # connection resets; HTTPException covers RemoteDisconnected /
            # BadStatusLine when the server hangs up mid-response.
            if attempt < FETCH_ATTEMPTS:
                delay = FETCH_BACKOFF_SECONDS[min(attempt - 1, len(FETCH_BACKOFF_SECONDS) - 1)]
                print(f"  {what} failed (attempt {attempt}/{FETCH_ATTEMPTS}): {exc}; retrying in {delay}s", file=sys.stderr)
                time.sleep(delay)
            else:
                print(f"  {what} failed after {FETCH_ATTEMPTS} attempts: {exc}", file=sys.stderr)
    return None


def _qrz_get_raw(params: dict[str, str]) -> bytes | None:
    url = QRZ_XML_URL + "?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={"User-Agent": "BKG-Roster-Builder/1.0"})
    return _fetch_with_retry(req, timeout=15, what="QRZ request")


def _qrz_get(params: dict[str, str]) -> ET.Element | None:
    raw = _qrz_get_raw(params)
    if raw is None:
        return None
    try:
        return ET.fromstring(raw)
    except ET.ParseError as exc:
        print(f"  QRZ XML parse failed: {exc}", file=sys.stderr)
        return None


def qrz_login(username: str, password: str) -> str | None:
    """Authenticate with QRZ XML API; returns a session key or None."""
    root = _qrz_get({"username": username, "password": password, "agent": "bkg.club-1.0"})
    if root is None:
        return None
    session = root.find("q:Session", QRZ_NS)
    if session is None:
        return None
    error = session.find("q:Error", QRZ_NS)
    if error is not None and error.text:
        print(f"  QRZ login error: {error.text}", file=sys.stderr)
        return None
    key = session.find("q:Key", QRZ_NS)
    return key.text if key is not None and key.text else None


def qrz_fetch_callsign(session_key: str, callsign: str, *, debug: bool = False) -> dict | None:
    """Look up a callsign. Returns {'current_call', 'image', 'grid', 'lat',
    'lon'}, or None if the lookup itself failed (network, parse, or
    session error) so the caller can fall back to the cached callsign.

    'current_call' is QRZ's canonical <call> element, which differs from the
    queried callsign for retired/aliased/vanity calls; callers use it to remap
    roster entries to the operator's current callsign. State and country come
    from the sheet's QTH column, not QRZ.
    """
    empty = {
        "current_call": None, "image": None,
        "grid": None, "lat": None, "lon": None,
    }
    raw = _qrz_get_raw({"s": session_key, "callsign": callsign})
    if raw is None:
        return None
    if debug:
        print(f"  QRZ response for {callsign}:", file=sys.stderr)
        for line in raw.decode("utf-8", errors="replace").splitlines():
            print(f"    {line}", file=sys.stderr)
    try:
        root = ET.fromstring(raw)
    except ET.ParseError as exc:
        print(f"  QRZ parse error for {callsign}: {exc}", file=sys.stderr)
        return None
    # Surface session-level errors (e.g. "Session Timeout", "Not subscribed").
    session = root.find("q:Session", QRZ_NS)
    session_error = None
    if session is not None:
        err = session.find("q:Error", QRZ_NS)
        if err is not None and err.text:
            session_error = err.text.strip()
            print(f"  QRZ session error for {callsign}: {session_error}", file=sys.stderr)
    call = root.find("q:Callsign", QRZ_NS)
    if call is None:
        # "Not found" is a real answer worth caching; anything else (session
        # timeout, subscription lapse, empty reply) is a failed lookup.
        if session_error and session_error.lower().startswith("not found"):
            return empty
        return None

    # QRZ returns the operator's canonical <call>, which may differ from the
    # queried callsign for retired/aliased/vanity calls. Use it as the
    # authoritative current callsign.
    current_call = None
    call_elem = call.find("q:call", QRZ_NS)
    if call_elem is not None and call_elem.text:
        candidate = call_elem.text.strip().upper()
        if re.fullmatch(r"[A-Z0-9/]+", candidate):
            current_call = candidate

    image = None
    image_elem = call.find("q:image", QRZ_NS)
    if image_elem is not None and image_elem.text:
        candidate = image_elem.text.strip()
        if candidate.startswith(("http://", "https://")):
            image = candidate

    # Maidenhead grid + coordinates, for the nearby-members map
    grid = None
    grid_elem = call.find("q:grid", QRZ_NS)
    if grid_elem is not None and grid_elem.text:
        text = grid_elem.text.strip().upper()
        if re.fullmatch(r"[A-R]{2}[0-9]{2}(?:[A-X]{2})?([0-9]{2})?", text):
            grid = text[:6]

    lat = lon = None
    lat_elem = call.find("q:lat", QRZ_NS)
    lon_elem = call.find("q:lon", QRZ_NS)
    if lat_elem is not None and lon_elem is not None and lat_elem.text and lon_elem.text:
        try:
            lat = float(lat_elem.text.strip())
            lon = float(lon_elem.text.strip())
        except ValueError:
            lat = lon = None
        if lat is not None and not (-90 <= lat <= 90 and -180 <= lon <= 180):
            lat = lon = None

    return {
        "current_call": current_call, "image": image,
        "grid": grid, "lat": lat, "lon": lon,
    }


def local_override_mugshot(callsign: str) -> str | None:
    """If images/mugshots-override/<callsign>.<ext> exists, copy it into the build
    mugshot dir and return its repo-relative path. Otherwise return None."""
    if not MUGSHOT_OVERRIDE_DIR.is_dir():
        return None
    for ext in MUGSHOT_EXTS:
        src = MUGSHOT_OVERRIDE_DIR / f"{callsign}{ext}"
        if src.is_file():
            MUGSHOT_DIR.mkdir(parents=True, exist_ok=True)
            dest = MUGSHOT_DIR / f"{callsign}{ext}"
            dest.write_bytes(src.read_bytes())
            return f"{MUGSHOT_REL_DIR}/{callsign}{ext}"
    return None


def load_mugshot_sources() -> dict[str, str]:
    """Image URL behind each mugshot file on disk.

    Lives beside the files in the workflow's mugshot cache (never
    committed), so an unchanged QRZ image isn't downloaded again.
    """
    try:
        data = json.loads(MUGSHOT_SOURCES_PATH.read_text())
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def save_mugshot_sources(sources: dict[str, str]) -> None:
    MUGSHOT_DIR.mkdir(parents=True, exist_ok=True)
    MUGSHOT_SOURCES_PATH.write_text(json.dumps(sources, indent=0, sort_keys=True) + "\n")


def download_mugshot(callsign: str, url: str, sources: dict[str, str]) -> str | None:
    """Download a QRZ profile image, reusing the file on disk when its URL
    hasn't changed. Returns the repo-relative path, or None on failure."""
    if sources.get(callsign) == url:
        existing = previous_mugshot(callsign)
        if existing:
            return existing
    parsed = urllib.parse.urlparse(url)
    suffix = Path(parsed.path).suffix.lower()
    if suffix not in MUGSHOT_EXTS:
        suffix = ".jpg"
    req = urllib.request.Request(url, headers={"User-Agent": "BKG-Roster-Builder/1.0"})
    data = _fetch_with_retry(req, timeout=20, what=f"QRZ image download for {callsign}")
    if not data:
        return None
    MUGSHOT_DIR.mkdir(parents=True, exist_ok=True)
    for old in MUGSHOT_DIR.glob(f"{callsign}.*"):
        old.unlink()
    filename = f"{callsign}{suffix}"
    (MUGSHOT_DIR / filename).write_bytes(data)
    sources[callsign] = url
    return f"{MUGSHOT_REL_DIR}/{filename}"


def previous_mugshot(callsign: str) -> str | None:
    """Last build's mugshot (restored by the workflow cache), if any."""
    for ext in MUGSHOT_EXTS:
        path = MUGSHOT_DIR / f"{callsign}{ext}"
        if path.is_file() and path.stat().st_size > 0:
            return f"{MUGSHOT_REL_DIR}/{callsign}{ext}"
    return None


def qth_location(qth: str) -> tuple[str | None, str | None]:
    """Sheet QTH -> (state code, country)."""
    text = qth.strip()
    if not text:
        return None, None
    code = STATE_CODES_BY_NAME.get(text.lower())
    if code is None and text.upper() in US_MAP_STATES:
        code = text.upper()
    if code:
        return code, "United States"
    return None, text


def mark_territory_ogs(members: list[dict]) -> None:
    """Badge the earliest member (lowest BKG #) in each map territory, in place.

    The first member in each US map state gets member['state_og'], and the
    first from each DX country gets member['country_og'] — the international
    counterpart of the state OG badge.
    """
    earliest: dict[tuple[str, str], dict] = {}
    for member in sorted(members, key=lambda m: m["number"]):
        bucket = member_map_bucket(member)
        if bucket and bucket not in earliest:
            earliest[bucket] = member
    for (kind, _territory), member in earliest.items():
        member["state_og" if kind == "state" else "country_og"] = True


def load_qrz_cache() -> dict[str, str]:
    """Read qrz-cache.json -> {SHEET CALLSIGN: CURRENT CALLSIGN}.

    Only callsigns that changed are stored.
    """
    if not QRZ_CACHE_PATH.is_file():
        return {}
    try:
        data = json.loads(QRZ_CACHE_PATH.read_text())
    except (OSError, ValueError) as exc:
        print(f"  Ignoring unreadable {QRZ_CACHE_PATH.name}: {exc}", file=sys.stderr)
        return {}
    if not isinstance(data, dict):
        return {}
    cache = {}
    for key, value in data.items():
        if key.startswith("_"):
            continue
        # Old format: {"current_call": ...}
        if isinstance(value, dict):
            value = value.get("current_call")
        if isinstance(value, str) and value:
            cache[key.upper()] = value.upper()
    return cache


def save_qrz_cache(cache: dict[str, str]) -> None:
    """Write the cache one entry per line, sorted, so commits diff cleanly."""
    lines = ["{", f'  "_about": {json.dumps(QRZ_CACHE_ABOUT)},']
    for key in sorted(cache):
        if cache[key] == key:
            continue  # unchanged calls need no entry
        lines.append(f"  {json.dumps(key)}: {json.dumps(cache[key])},")
    lines[-1] = lines[-1].rstrip(",")
    lines.append("}")
    QRZ_CACHE_PATH.write_text("\n".join(lines) + "\n")


def annotate_qrz(members: list[dict]) -> None:
    """Set member['state'], member['country'], member['state_og'],
    member['country_og'], member['grid'/'lat'/'lon'] and member['mugshot_path']
    in place.

    State and country come from the sheet's QTH column. Every member is
    looked up live on QRZ for their current callsign, mugshot and
    coordinates. qrz-cache.json stores only changed callsigns, used when a
    lookup fails; that member then keeps last build's mugshot file but has
    no coordinates this build. Mugshots download only when the QRZ image
    URL changes. Requires env vars QRZ_USERNAME and
    QRZ_PASSWORD (an XML-subscription QRZ account).
    """
    for member in members:
        member["state"], member["country"] = qth_location(member.get("qth", ""))
        member["state_og"] = False
        member["country_og"] = False
        member["mugshot_path"] = None
        member["grid"] = None
        member["lat"] = None
        member["lon"] = None

    cache = load_qrz_cache()
    sources = load_mugshot_sources()
    print(f"  QRZ cache: {len(cache)} callsigns; fetching {len(members)} live", file=sys.stderr)

    session_key = None
    if members:
        username = os.environ.get("QRZ_USERNAME")
        password = os.environ.get("QRZ_PASSWORD")
        if not username or not password:
            print("  QRZ_USERNAME/QRZ_PASSWORD not set; using cached callsigns only", file=sys.stderr)
        else:
            session_key = qrz_login(username, password)
            if not session_key:
                print("  QRZ login failed; using cached callsigns only", file=sys.stderr)

    fetched = failed = streak = 0
    debug_done = False
    for member in members:
        key = member["callsign"].upper()
        info = None
        if session_key:
            info = qrz_fetch_callsign(session_key, key, debug=not debug_done)
            debug_done = True
            if info is None:
                failed += 1
                streak += 1
                if streak >= QRZ_MAX_CONSECUTIVE_FAILURES:
                    print(f"  QRZ failed {streak} times in a row; using cache", file=sys.stderr)
                    session_key = None
            else:
                fetched += 1
                streak = 0
                cache[key] = info.get("current_call") or key

        # Remap to the operator's current callsign if QRZ reports a different
        # canonical <call> (e.g. after a vanity/retired-call change).
        current_call = info.get("current_call") if info else cache.get(key)
        if current_call and current_call != key:
            print(f"  Callsign updated: {member['callsign']} -> {current_call}", file=sys.stderr)
            # Keep the sheet call for sponsor matching.
            member["sheet_callsign"] = member["callsign"]
            member["callsign"] = current_call

        if info:
            member["grid"] = info.get("grid")
            member["lat"] = info.get("lat")
            member["lon"] = info.get("lon")
        override = local_override_mugshot(member["callsign"])
        if override:
            member["mugshot_path"] = override
        elif info is None:
            member["mugshot_path"] = previous_mugshot(member["callsign"])
        elif info.get("image"):
            member["mugshot_path"] = download_mugshot(
                member["callsign"], info["image"], sources
            ) or previous_mugshot(member["callsign"])

    if members:
        print(f"  QRZ lookups: {fetched} fetched, {failed} failed", file=sys.stderr)
    save_qrz_cache(cache)
    save_mugshot_sources(sources)

    resolved = sum(1 for m in members if member_map_bucket(m))
    dx = sum(1 for m in members if (member_map_bucket(m) or ("",))[0] == "dx")
    mugshots = sum(1 for m in members if m.get("mugshot_path"))
    print(
        f"  Sheet QTH placed {resolved}/{len(members)} members on the map ({dx} international)",
        file=sys.stderr,
    )
    unknown = sorted({m["qth"] for m in members if m.get("qth") and not member_map_bucket(m)})
    if unknown:
        print(f"  Unrecognized QTH values: {unknown}", file=sys.stderr)
    print(f"  Mugshot resolved for {mugshots}/{len(members)} members (local overrides preferred)", file=sys.stderr)

    apply_location_overrides(members, load_location_overrides())
    mark_territory_ogs(members)


NAME_SUFFIXES = {"jr", "sr", "ii", "iii", "iv"}
NAME_TITLES = {"dr", "mr", "mrs", "ms", "rev"}


def normalize_name_case(name: str) -> str:
    """Title-case a name typed in ALL CAPS or lowercase.

    Mixed-case entries (McDonald, DeVon, QRS Forrest) are left alone.
    Handles hyphens and apostrophes: MARY-JO O'BRIEN -> Mary-Jo O'Brien.
    """
    if name.isupper() or name.islower():
        return re.sub(r"[A-Za-z]+", lambda m: m.group(0).capitalize(), name)
    return name


def first_name_initial(name: str) -> str:
    """Return 'First L' from a roster name, normalizing sheet formatting.

    Handles extra whitespace, ALL CAPS / lowercase, a quoted or parenthesized
    nickname (Robert "Bob" Smith -> Bob S), 'Last, First', titles, and
    suffixes like 'Jr'.
    """
    if not name:
        return ""
    name = normalize_name_case(" ".join(name.split()))
    # Prefer a nickname: Robert "Bob" Smith / Robert (Bob) Smith -> Bob Smith
    nick = re.search(r'["\u201c\u201d(]\s*([^"\u201c\u201d()]+?)\s*["\u201c\u201d)]', name)
    if nick:
        name = f"{nick.group(1)} {name[:nick.start()]} {name[nick.end():]}"
    # 'Smith, John' -> 'John Smith' (but not 'John Smith, Jr')
    if "," in name:
        head, tail = (part.strip() for part in name.split(",", 1))
        if tail and tail.rstrip(".").lower() not in NAME_SUFFIXES:
            name = f"{tail} {head}"
    parts = [p.strip(",. ") for p in name.split()]
    parts = [p for p in parts if p]
    parts = [p for p in parts if p.lower() not in NAME_TITLES]
    if not parts:
        return ""
    first = parts[0]
    if len(parts) == 1:
        return first
    last_candidates = [p for p in parts[1:] if p.lower() not in NAME_SUFFIXES]
    if not last_candidates:
        return first
    initial = re.sub(r"[^A-Za-z]", "", last_candidates[-1])[:1].upper()
    return f"{first} {initial}" if initial else first


def territory_og_badge_html(member: dict) -> str:
    """Corner ribbon for the first BKG member in a US state or DX country.

    State ribbons show the state code ("UT OG"); country ribbons show the
    country's flag emoji ("🇨🇦 OG") so long DXCC names don't blow out the
    ribbon, with the full name in the hover title.
    """
    if member.get("state_og"):
        label = html_escape(member.get("state") or "")
        title = label
    elif member.get("country_og"):
        country = (member.get("country") or "").strip()
        label = COUNTRY_FLAGS.get(country.lower(), "🌍")
        title = html_escape(country)
    else:
        return ""
    return f'\n                    <div class="state-og-badge" title="{title} OG">{label} OG</div>'


def mugshot_inner_html(member: dict) -> str:
    path = member.get("mugshot_path")
    if path:
        callsign = html_escape(member["callsign"])
        src = html_escape(path)
        return (
            f'<img class="mugshot-photo" src="{src}" alt="{callsign} QRZ profile photo" loading="lazy">'
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


def render_member_card(member: dict, *, is_og: bool, is_new: bool) -> str:
    callsign = html_escape(member["callsign"])
    name = html_escape(member["name"])
    number = f"BKG #{member['number']:03d}"
    badge_html = ""
    if is_og:
        badge_html = '\n                    <div class="og-badge">OG</div>'
    elif is_new:
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


def render_roster_block(members: list[dict]) -> str:
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
                    is_og=member["number"] in OG_BADGE_NUMBERS,
                    is_new=member["number"] in new_numbers,
                )
            )
    next_number = max(m["number"] for m in members) + 1
    cards.append(render_ghost_card(next_number))
    return "\n\n".join(cards)


def zero_for_o(call: str) -> str:
    """Fold letter O into digit 0."""
    return call.upper().replace("O", "0")


def resolve_sponsors(members: list[dict]) -> None:
    """Resolve each member's raw "Sponsor" value to the sponsoring member in place.

    A sponsor may be recorded as a callsign ("KI7QCF") or a BKG number ("12",
    "BKG12", "BKG #12"). Sets member['sponsor_member'] to the matched member's
    dict, or None for a root / unrecognized sponsor. Runs after annotate_qrz()
    so a sponsor cell may name either the sheet callsign or the QRZ-updated
    one. A letter O typed for a digit 0 ("NBOE") still matches.
    """
    by_call = {}
    for m in members:
        for call in (m.get("sheet_callsign"), m["callsign"]):
            if call:
                by_call[call.upper()] = m
                by_call.setdefault(zero_for_o(call), m)
    by_num = {m["number"]: m for m in members}
    for member in members:
        raw = (member.get("sponsor_raw") or "").strip()
        if not raw:
            continue
        # Prefer an exact callsign match before parsing a number, so a callsign
        # like "K5OHY" isn't mistaken for member number 5.
        sponsor = by_call.get(raw.upper()) or by_call.get(zero_for_o(raw))
        if sponsor is None:
            num = parse_member_number(raw)
            sponsor = by_num.get(num) if num else None
        if sponsor is None:
            print(f"  Unrecognized sponsor for {member['callsign']}: {raw!r}", file=sys.stderr)
        elif sponsor is member:
            print(f"  Ignoring self-sponsor for {member['callsign']}", file=sys.stderr)
        else:
            member["sponsor_member"] = sponsor
    resolved = sum(1 for m in members if m.get("sponsor_member"))
    print(f"  Resolved sponsor for {resolved}/{len(members)} members", file=sys.stderr)


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
    return json.dumps(nodes, separators=(",", ":"))


def render_map_data(members: list[dict]) -> str:
    """Build the JSON map data consumed by the territory map (#bkg-map-data):

    {"states": {state_abbr: [{"call", "name", "num"}, ...]},
     "dx": {country: {"flag": "🇨🇦", "members": [{"call", "name", "num"}, ...]}}}

    US members land in "states" (keyed by QRZ state); international members
    land in "dx" (keyed by QRZ country) and render in the map's DX inset box.
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
    return json.dumps(data, separators=(",", ":"))


def grid_to_latlon(grid: str) -> tuple[float, float] | None:
    """Center of a 4- or 6-char Maidenhead grid square."""
    if not re.fullmatch(r"[A-R]{2}[0-9]{2}(?:[A-X]{2})?", grid or ""):
        return None
    lon = (ord(grid[0]) - ord("A")) * 20 - 180 + int(grid[2]) * 2
    lat = (ord(grid[1]) - ord("A")) * 10 - 90 + int(grid[3])
    if len(grid) == 6:
        lon += (ord(grid[4]) - ord("A")) * (2 / 24) + 1 / 24
        lat += (ord(grid[5]) - ord("A")) * (1 / 24) + 0.5 / 24
    else:
        lon += 1.0
        lat += 0.5
    return (round(lat, 4), round(lon, 4))


def member_latlon(member: dict) -> tuple[float, float] | None:
    """Coarse public coordinates for a member, or None.

    Grid-square center when QRZ lists a grid (deliberately coarse), else the
    QRZ lat/lon rounded to 2 decimals (~1 km). Members with a location
    override have had their QRZ coords cleared, so they come back None.
    """
    grid = member.get("grid")
    latlon = grid_to_latlon(grid) if grid else None
    if latlon is None and member.get("lat") is not None and member.get("lon") is not None:
        latlon = (round(member["lat"], 2), round(member["lon"], 2))
    return latlon


def render_geo_data(members: list[dict]) -> str:
    """Build the JSON geo data consumed by nearby.html (#bkg-geo-data):

    [{"call", "name", "num", "lat", "lon", "grid"}, ...] — one entry per
    member with a usable location. Coordinates are grid-square centers
    (QRZ-public and deliberately coarse); a member with QRZ lat/lon but no
    grid gets those coords rounded to 2 decimals (~1 km) instead. Members
    with a location override are excluded — their QRZ coords are stale.
    """
    entries = []
    for member in sorted(members, key=lambda m: m["number"]):
        grid = member.get("grid")
        latlon = member_latlon(member)
        if latlon is None:
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
    return json.dumps(entries, separators=(",", ":"))


def update_nearby(members: list[dict], output_dir: Path) -> None:
    """Inject the geo JSON from the source template into the public build."""
    html = NEARBY_PATH.read_text()
    html = replace_between(
        html,
        "<!-- GEO_DATA:START -->",
        "<!-- GEO_DATA:END -->",
        render_geo_data(members),
    )
    (output_dir / "nearby.html").write_text(html)


# Join Date cell formats seen (or plausible) on the roster sheet. Google
# Sheets exports dates as typed, so a mix of these is expected.
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
    "lat"/"lon" the same coarse coordinates nearby.html gets (present only
    when QRZ had a grid or lat/lon; the page falls back to the state).
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
    return json.dumps(entries, separators=(",", ":"), ensure_ascii=False)


def update_outbreak(members: list[dict], output_dir: Path) -> None:
    """Inject the outbreak JSON from the source template into the public build."""
    html = OUTBREAK_PATH.read_text()
    html = replace_between(
        html,
        "<!-- OUTBREAK_DATA:START -->",
        "<!-- OUTBREAK_DATA:END -->",
        render_outbreak_data(members),
    )
    (output_dir / "outbreak.html").write_text(html)


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
    html = INDEX_PATH.read_text()
    roster = render_roster_block(members)
    html = replace_between(
        html,
        "<!-- ROSTER:START - auto-generated by scripts/build-roster.py from Google Sheet -->",
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
    (output_dir / "index.html").write_text(html)


def update_tree(members: list[dict], output_dir: Path) -> None:
    """Inject the downline JSON from the source template into the public build."""
    html = TREE_PATH.read_text()
    html = replace_between(
        html,
        "<!-- DOWNLINE_DATA:START -->",
        "<!-- DOWNLINE_DATA:END -->",
        render_downline_data(members),
    )
    (output_dir / "tree.html").write_text(html)


def og_note_tags(member: dict) -> list[str]:
    """OG labels for a callsign note, mirroring the roster badges.

    The founder and OG_BADGE_NUMBERS get "OG"; the first member in a US
    state or DX country gets "UT OG" / "Canada OG".
    """
    tags = []
    if member["number"] == 1 or member["number"] in OG_BADGE_NUMBERS:
        tags.append("OG")
    if member.get("state_og") and member.get("state"):
        tags.append(f"{member['state']} OG")
    elif member.get("country_og") and member.get("country"):
        tags.append(f"{member['country'].strip()} OG")
    return tags


def render_members_txt(members: list[dict]) -> str:
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
    lines = [
        "# BKG Callsign Notes for Ham2K PoLo",
        f"# Generated: {now}",
        "# Do not edit manually - this file is auto-generated",
        "",
    ]
    for member in sorted(members, key=lambda m: m["callsign"]):
        if member.get("name_override"):
            label = member["name"]
        else:
            label = first_name_initial(member["name"])
        line = f"{member['callsign']} 🤜 {label} BKG #{member['number']}"
        tags = og_note_tags(member)
        if tags:
            line += f" ({', '.join(tags)})"
        lines.append(line)
    lines.append("")
    return "\n".join(lines)


def stage_public_files(output_dir: Path) -> None:
    """Copy only site files and image assets; cache metadata stays in the repo."""
    for relative in PUBLIC_FILES:
        source = REPO_ROOT / relative
        destination = output_dir / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
    for relative in PHOTO_DIRS:
        source_dir = REPO_ROOT / relative
        if not source_dir.is_dir():
            continue
        for source in source_dir.iterdir():
            if not source.is_file() or source.name.startswith(".") or source.suffix.lower() not in MUGSHOT_EXTS:
                continue
            destination = output_dir / relative / source.relative_to(source_dir)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)


def build(output_dir: Path) -> None:
    print(f"Fetching roster from {SHEET_CSV_URL}")
    members = parse_members(fetch_csv())
    validate_members(members)
    print(f"Parsed {len(members)} members (BKG #{members[0]['number']:03d}–#{members[-1]['number']:03d})")

    overrides = load_name_overrides()
    if overrides:
        print(f"Applying {len(overrides)} name override(s) from {NAME_OVERRIDES_PATH.name}")
        apply_name_overrides(members, overrides)

    print("Looking up location + mugshot for each member via QRZ XML API")
    annotate_qrz(members)
    validate_members(members)

    # After QRZ so updated callsigns resolve.
    print("Resolving sponsors for the downline tree")
    resolve_sponsors(members)
    state_ogs = sum(1 for m in members if m.get("state_og"))
    country_ogs = sum(1 for m in members if m.get("country_og"))
    print(f"Marked {state_ogs} state OG(s) + {country_ogs} country OG(s) on the map")

    # Copy after QRZ so newly downloaded and overridden photos are included.
    stage_public_files(output_dir)
    update_index(members, output_dir)
    print(f"Updated {INDEX_PATH.name}")

    update_tree(members, output_dir)
    print(f"Updated {TREE_PATH.name}")

    update_nearby(members, output_dir)
    print(f"Updated {NEARBY_PATH.name}")

    update_outbreak(members, output_dir)
    print(f"Updated {OUTBREAK_PATH.name}")

    (output_dir / "members.txt").write_text(render_members_txt(members))
    print("Wrote members.txt")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output-dir", type=Path, default=REPO_ROOT / "dist",
        help="public build directory (default: dist/)",
    )
    output_dir = parser.parse_args(argv).output_dir.resolve()
    try:
        source_root = REPO_ROOT.resolve()
        if (output_dir == source_root or output_dir in source_root.parents
                or (output_dir.is_relative_to(source_root) and output_dir != source_root / "dist")):
            raise ValueError("Output must be dist/ or a directory outside the source repository")
        # Never let a failed generation leave a half-built output directory.
        # Source templates, QRZ fallback cache, and the mugshot cache remain
        # in their existing locations; only public output is staged here.
        output_dir.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(
            prefix=f".{output_dir.name}-", dir=output_dir.parent
        ) as temporary:
            staged = Path(temporary) / "public"
            staged.mkdir()
            build(staged)
            if output_dir.exists():
                shutil.rmtree(output_dir)
            staged.rename(output_dir)
    except Exception as exc:
        print(f"ERROR: Build failed: {exc}", file=sys.stderr)
        return 1
    print(f"Built public site in {output_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
