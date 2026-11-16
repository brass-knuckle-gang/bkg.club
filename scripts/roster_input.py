"""Strict adapter for the administration's public V2 roster contract.

Validate reviewed membership and public asset metadata before handing members
to the renderer. A cached response is reused only with a matching HTTP 304.
"""

import hashlib
import http.client
import json
import math
import os
from datetime import datetime, timezone
from pathlib import Path
import re
import tempfile
import urllib.error
import urllib.parse
import urllib.request


MAX_ROSTER_BYTES = 8 * 1024 * 1024
MAX_BKG_NUMBER = 9007199254740991
EXPORT_TIMEOUT_SECONDS = 30
ENVELOPE_KEYS = {"schema_version", "generated_at", "content_hash", "members", "og_assignments"}
MEMBER_KEYS = {"bkg_number", "callsign", "name", "qth", "qso_date", "sponsor_bkg_number", "map_location", "photo"}
GRID_PATTERN = re.compile(r"[A-R]{2}[0-9]{2}(?:[A-X]{2})?\Z")
# Degrees, about 11 m: accepts a center rounded to four decimal places (as V1
# published them) but nothing else inside a grid that is kilometres across.
GRID_CENTER_TOLERANCE = 1e-4
PHOTO_KEYS = {"url", "sha256", "content_type", "byte_length"}
MAX_PUBLIC_PHOTO_BYTES = 1024 * 1024
OG_KEYS = {"region_key", "region_label", "bkg_number"}
CALLSIGN_PATTERN = re.compile(r"[A-Za-z0-9]+\Z")
UTC_TIMESTAMP_PATTERN = re.compile(
    r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|\+00:00)\Z"
)


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def _object(value, keys: set[str], label: str) -> None:
    _require(type(value) is dict, f"{label} must be an object")
    _require(set(value) == keys, f"{label} has missing or additional fields")


def _number(value, label: str) -> None:
    _require(type(value) is int and 1 <= value <= MAX_BKG_NUMBER, f"Invalid {label}")


def _string(value, label: str, *, nonempty: bool = False, line_safe: bool = False) -> None:
    _require(type(value) is str, f"{label} must be a string")
    _require(not nonempty or bool(value), f"{label} must not be empty")
    # Preserve reviewed Unicode and spacing verbatim, while rejecting controls
    # that could create extra records in members.txt or misleading public text.
    _require(not any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in value),
             f"{label} contains control characters")
    _require(not line_safe or not any(char in "\u2028\u2029" for char in value),
             f"{label} contains line separators")
    try:
        value.encode("utf-8")
    except UnicodeEncodeError:
        raise ValueError(f"{label} contains invalid Unicode") from None


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        _require(key not in result, "Duplicate JSON object field")
        result[key] = value
    return result


def _invalid_constant(_value):
    raise ValueError("Non-finite numbers are not valid roster JSON")


def grid_center(grid: str) -> tuple[float, float]:
    """(lat, lon) of the center of a valid 4- or 6-character Maidenhead grid."""
    lat = -90 + (ord(grid[1]) - ord("A")) * 10 + int(grid[3])
    lon = -180 + (ord(grid[0]) - ord("A")) * 20 + int(grid[2]) * 2
    if len(grid) == 4:
        return lat + 0.5, lon + 1
    return lat + (ord(grid[5]) - ord("A") + 0.5) / 24, lon + (ord(grid[4]) - ord("A") + 0.5) / 12


def _map_location(value, number: int) -> None:
    if value is None:
        return
    _object(value, {"grid", "lat", "lon"}, "Member map_location")
    grid = value["grid"]
    _require(type(grid) is str and bool(GRID_PATTERN.fullmatch(grid)), "Invalid map_location grid")
    for key, limit in (("lat", 90), ("lon", 180)):
        coordinate = value[key]
        _require(type(coordinate) in (int, float) and math.isfinite(coordinate) and abs(coordinate) <= limit,
                 f"Invalid map_location {key}")
    # The contract publishes only the grid's center. Anything else is a more
    # precise position than the member's public grid, or a dot drawn somewhere
    # other than the grid it is grouped and searched under.
    for coordinate, center in zip((value["lat"], value["lon"]), grid_center(grid)):
        _require(math.isclose(coordinate, center, rel_tol=0, abs_tol=GRID_CENTER_TOLERANCE),
                 f"map_location coordinates are not the center of {grid} (BKG #{number})")


def _photo(value) -> None:
    if value is None:
        return
    _object(value, PHOTO_KEYS, "Member photo")
    digest = value["sha256"]
    _require(type(digest) is str and bool(re.fullmatch(r"[0-9a-f]{64}", digest)), "Invalid photo sha256")
    _require(value["content_type"] == "image/webp", "Invalid photo content_type")
    _require(type(value["byte_length"]) is int and 1 <= value["byte_length"] <= MAX_PUBLIC_PHOTO_BYTES,
             "Invalid photo byte_length")
    url = value["url"]
    _require(type(url) is str and len(url) <= 2048 and
             not any(char.isspace() or ord(char) < 32 or ord(char) == 127 for char in url),
             "Invalid photo URL")
    try:
        parsed = urllib.parse.urlsplit(url)
        _require(parsed.scheme == "https" and bool(parsed.hostname) and parsed.port != 0
                 and parsed.username is None and parsed.password is None
                 and not parsed.query and not parsed.fragment
                 and parsed.path == f"/api/roster/photos/{digest}.webp", "Invalid photo URL")
    except ValueError:
        raise ValueError("Invalid photo URL") from None


def parse_roster_json(text: str, *, exclude_invalid_callsigns: bool = False,
                      exclusions: list | None = None) -> tuple[dict, list[dict]]:
    """Validate the V2 contract, then adapt reviewed members.

    By default the original envelope is returned without normalizing strings or
    array order. Publication can explicitly exclude invalid callsigns, only after
    validating the complete source contract and hash. That projection has its own
    hash, unresolved sponsors for excluded members, and no reassigned OGs.
    Member ``join_date`` is the stored QSO date; sponsor relationships
    are resolved only through stable BKG numbers. ``og_regions`` retains the
    export's stored region labels even when its holder currently lives elsewhere.
    """
    _require(type(text) is str, "Roster JSON input must be UTF-8 text")
    try:
        _require(len(text.encode("utf-8")) <= MAX_ROSTER_BYTES, "Roster JSON exceeds size limit")
        envelope = json.loads(text, object_pairs_hook=_unique_object, parse_constant=_invalid_constant)
    except (json.JSONDecodeError, RecursionError, UnicodeEncodeError):
        raise ValueError("Malformed roster JSON") from None
    _object(envelope, ENVELOPE_KEYS, "Roster envelope")
    _require(envelope["schema_version"] == "2", "Unsupported roster schema_version")
    generated_at = envelope["generated_at"]
    _require(type(generated_at) is str and bool(UTC_TIMESTAMP_PATTERN.fullmatch(generated_at)),
             "generated_at must be an ISO UTC timestamp")
    try:
        timestamp = datetime.fromisoformat(generated_at.replace("Z", "+00:00"))
        _require(timestamp.utcoffset() == timezone.utc.utcoffset(timestamp),
                 "generated_at must be an ISO UTC timestamp")
    except ValueError:
        raise ValueError("generated_at must be an ISO UTC timestamp") from None
    _require(type(envelope["content_hash"]) is str and
             bool(re.fullmatch(r"sha256:[0-9a-f]{64}", envelope["content_hash"])),
             "Invalid roster content_hash")

    rows = envelope["members"]
    assignments = envelope["og_assignments"]
    _require(type(rows) is list, "members must be an array")
    _require(bool(rows), "Roster contains no active members")
    _require(type(assignments) is list, "og_assignments must be an array")
    members = []
    by_number = {}
    callsigns = set()
    previous_number = 0
    for row in rows:
        _object(row, MEMBER_KEYS, "Member")
        number = row["bkg_number"]
        _number(number, "member bkg_number")
        _require(number > previous_number, "Members must have unique ascending bkg_number values")
        previous_number = number
        for key in ("callsign", "name", "qth", "qso_date"):
            _string(row[key], f"Member {key}", line_safe=key == "name")
        callsign = row["callsign"]
        _require(exclude_invalid_callsigns or bool(CALLSIGN_PATTERN.fullmatch(callsign)),
                 f"Invalid member callsign (BKG #{number})")
        _require(callsign.upper() not in callsigns, "Duplicate member callsign")
        callsigns.add(callsign.upper())
        sponsor = row["sponsor_bkg_number"]
        if sponsor is not None:
            _number(sponsor, "sponsor_bkg_number")
        location = row["map_location"]
        _map_location(location, number)
        _photo(row["photo"])
        member = {
            "number": number,
            "callsign": callsign,
            "name": row["name"],
            "qth": row["qth"],
            "join_date": row["qso_date"],
            "sponsor_bkg_number": sponsor,
            "sponsor_member": None,
            "og_regions": [],
            "map_location": location,
            "photo": row["photo"],
            "grid": location["grid"] if location else None,
            "lat": location["lat"] if location else None,
            "lon": location["lon"] if location else None,
            "mugshot_path": None,
        }
        members.append(member)
        by_number[number] = member

    for member in members:
        sponsor_number = member["sponsor_bkg_number"]
        if sponsor_number is not None:
            _require(sponsor_number in by_number, "Sponsor references a missing active member")
            _require(sponsor_number != member["number"], "Member cannot sponsor itself")
            member["sponsor_member"] = by_number[sponsor_number]
    # Iterative traversal handles large valid rosters without recursion limits.
    resolved = set()
    for member in members:
        path = set()
        current = member
        while current is not None and current["number"] not in resolved:
            number = current["number"]
            _require(number not in path, "Sponsor relationships contain a cycle")
            path.add(number)
            current = current["sponsor_member"]
        resolved.update(path)

    previous_region = None
    for assignment in assignments:
        _object(assignment, OG_KEYS, "OG assignment")
        for key in ("region_key", "region_label"):
            _string(assignment[key], f"OG {key}", nonempty=True, line_safe=key == "region_label")
        region_key = assignment["region_key"]
        _require(previous_region is None or previous_region < region_key,
                 "OG assignments must have unique ascending region_key values")
        previous_region = region_key
        number = assignment["bkg_number"]
        _number(number, "OG bkg_number")
        _require(number in by_number, "OG references a missing active member")
        by_number[number]["og_regions"].append({
            "region_key": region_key,
            "region_label": assignment["region_label"],
        })

    _require(envelope["content_hash"] == roster_content_hash(envelope), "Roster content hash mismatch")
    if exclude_invalid_callsigns:
        excluded = {row["bkg_number"] for row in rows if not CALLSIGN_PATTERN.fullmatch(row["callsign"])}
        if excluded:
            projected = dict(envelope)
            projected["members"] = [dict(row, sponsor_bkg_number=(
                None if row["sponsor_bkg_number"] in excluded else row["sponsor_bkg_number"]
            )) for row in rows if row["bkg_number"] not in excluded]
            _require(bool(projected["members"]), "Roster contains no usable members")
            projected["og_assignments"] = [row for row in assignments if row["bkg_number"] not in excluded]
            projected["content_hash"] = roster_content_hash(projected)
            result = parse_roster_json(json.dumps(projected, ensure_ascii=False))
            if exclusions is not None:
                exclusions.extend({"bkg_number": number, "reason": "Invalid callsign characters"}
                                  for number in sorted(excluded))
            return result
    return envelope, members


def roster_content_hash(envelope: dict) -> str:
    payload = {key: envelope[key] for key in ("schema_version", "members", "og_assignments")}
    canonical = json.dumps(payload, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return "sha256:" + hashlib.sha256(canonical).hexdigest()


class _NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def _decode_roster(data: bytes) -> str:
    _require(len(data) <= MAX_ROSTER_BYTES, "Roster JSON exceeds size limit")
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        raise ValueError("Roster JSON is not UTF-8") from None


def read_roster_json(path: str | Path | None = None, *, url: str | None = None,
                     environ=None, cache_dir=None,
                     exclude_invalid_callsigns: bool = False) -> str:
    """Read a local roster file or an HTTPS roster endpoint.

    With no file or explicit URL, ``ROSTER_EXPORT_URL`` is required. Network
    errors omit request and response details.
    """
    _require(not (path is not None and url is not None), "Select one roster JSON file or URL")
    if path is not None:
        _require(cache_dir is None, "Snapshot caching requires a URL source")
        try:
            with Path(path).open("rb") as source:
                return _decode_roster(source.read(MAX_ROSTER_BYTES + 1))
        except OSError:
            raise OSError("Unable to read roster JSON file") from None

    environment = os.environ if environ is None else environ
    endpoint = url if url is not None else environment.get("ROSTER_EXPORT_URL")
    _require(type(endpoint) is str and bool(endpoint), "ROSTER_EXPORT_URL is required for JSON input")
    try:
        parsed = urllib.parse.urlsplit(endpoint)
        _require(parsed.scheme == "https" and bool(parsed.hostname) and parsed.port != 0 and
                 parsed.username is None and parsed.password is None and not parsed.fragment and not parsed.query and
                 not any(char.isspace() or ord(char) < 32 or ord(char) == 127 for char in endpoint),
                 "Roster export URL must be HTTPS without user info, query, or fragment")
    except ValueError:
        raise ValueError("Roster export URL must be HTTPS without user info, query, or fragment") from None
    headers = {"Accept": "application/json", "User-Agent": "BKG-Roster-Builder/1.0"}
    auth_fields = (("CF_ACCESS_CLIENT_ID", "CF-Access-Client-Id"),
                   ("CF_ACCESS_CLIENT_SECRET", "CF-Access-Client-Secret"))
    # An Access-protected endpoint may use a complete service token pair.
    authenticated = any(environment.get(key) is not None for key, _ in auth_fields)
    for key, header in auth_fields if authenticated else ():
        credential = environment.get(key)
        _require(type(credential) is str and bool(credential.strip()) and
                 all(32 <= ord(char) <= 126 for char in credential),
                 "Access authentication requires a complete valid credential pair")
        headers[header] = credential
    request = urllib.request.Request(endpoint, headers=headers)
    # Never trust a restored cache without checking its version, full contract,
    # hash and endpoint binding. A bad cache causes an unconditional fetch;
    # transport failures never serve a stale snapshot.
    cached_text = None
    cached_etag = None
    endpoint_hash = hashlib.sha256(endpoint.encode("utf-8")).hexdigest()
    cache_file = Path(cache_dir) / "v2.json" if cache_dir is not None else None
    if cache_file is not None:
        try:
            _require(not cache_file.is_symlink(), "Unsafe roster cache")
            with cache_file.open("rb") as source:
                data = source.read(MAX_ROSTER_BYTES * 2 + 4097)
            _require(len(data) <= MAX_ROSTER_BYTES * 2 + 4096, "Roster cache exceeds size limit")
            cached = json.loads(data.decode("utf-8"))
            _object(cached, {"endpoint_sha256", "etag", "snapshot"}, "Roster cache")
            _require(cached["endpoint_sha256"] == endpoint_hash, "Roster cache endpoint changed")
            parse_roster_json(cached["snapshot"], exclude_invalid_callsigns=exclude_invalid_callsigns)
            # Conditional requests always use the verified source hash, never
            # the hash of a filtered publication projection.
            cached_envelope = json.loads(cached["snapshot"])
            _require(cached["etag"] == f'W/"{cached_envelope["content_hash"]}"', "Roster cache ETag mismatch")
            cached_text, cached_etag = cached["snapshot"], cached["etag"]
            request.add_header("If-None-Match", cached_etag)
        except (OSError, ValueError, TypeError, KeyError):
            pass

    def not_modified(response):
        _require(cached_text is not None and response.headers.get("ETag") == cached_etag,
                 "Roster 304 requires a matching validated cache and ETag")
        # Revalidate on every use, including responses with no JSON body.
        parse_roster_json(cached_text, exclude_invalid_callsigns=exclude_invalid_callsigns)
        return cached_text

    try:
        with urllib.request.build_opener(_NoRedirects()).open(request, timeout=EXPORT_TIMEOUT_SECONDS) as response:
            if response.getcode() == 304:
                return not_modified(response)
            _require(response.getcode() == 200, "Roster export did not return HTTP 200")
            text = _decode_roster(response.read(MAX_ROSTER_BYTES + 1))
            if cache_file is not None:
                parse_roster_json(text, exclude_invalid_callsigns=exclude_invalid_callsigns)
                envelope = json.loads(text)
                etag = response.headers.get("ETag")
                expected_etag = f'W/"{envelope["content_hash"]}"'
                _require(etag is None or etag == expected_etag, "Roster response ETag mismatch")
                cache_file.parent.mkdir(parents=True, exist_ok=True)
                with tempfile.TemporaryDirectory(prefix=".snapshot-", dir=cache_file.parent) as temporary:
                    staged = Path(temporary) / "cache.json"
                    staged.write_text(json.dumps({"endpoint_sha256": endpoint_hash,
                                                 "etag": etag, "snapshot": text}, ensure_ascii=False),
                                      encoding="utf-8")
                    staged.replace(cache_file)
            return text
    except urllib.error.HTTPError as error:
        try:
            if error.code == 304:
                return not_modified(error)
        finally:
            error.close()
        raise OSError(f"Unable to fetch roster JSON (HTTP {error.code})") from None
    except (urllib.error.URLError, OSError, http.client.HTTPException, UnicodeError):
        raise OSError("Unable to fetch roster JSON") from None
