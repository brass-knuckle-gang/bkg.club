"""Strict adapter for the anonymous public roster v1 contract.

The exporter contains reviewed membership data, not QRZ enrichment. Validation
is deliberately completed before returning any members to the renderer. This
module never consults Sheets, caches, examples, or another roster on failure.
"""

import hashlib
import http.client
import json
import os
from datetime import datetime, timezone
from pathlib import Path
import re
import urllib.error
import urllib.parse
import urllib.request


MAX_ROSTER_BYTES = 8 * 1024 * 1024
MAX_BKG_NUMBER = 9007199254740991
EXPORT_TIMEOUT_SECONDS = 30
ENVELOPE_KEYS = {"schema_version", "generated_at", "content_hash", "members", "og_assignments"}
MEMBER_KEYS = {"bkg_number", "callsign", "name", "qth", "qso_date", "sponsor_bkg_number"}
OG_KEYS = {"region_key", "region_label", "bkg_number"}
CALLSIGN_PATTERN = re.compile(r"[A-Za-z0-9]+(?:/[A-Za-z0-9]+)*\Z")
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


def parse_roster_json(text: str) -> tuple[dict, list[dict]]:
    """Validate the entire v1 envelope, then adapt reviewed active members.

    The original envelope is returned without normalizing its strings or array
    order. Member ``join_date`` is the stored QSO date; sponsor relationships
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
    _require(envelope["schema_version"] == "1", "Unsupported roster schema_version")
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
        _require(bool(CALLSIGN_PATTERN.fullmatch(callsign)), "Invalid member callsign")
        _require(callsign.upper() not in callsigns, "Duplicate member callsign")
        callsigns.add(callsign.upper())
        sponsor = row["sponsor_bkg_number"]
        if sponsor is not None:
            _number(sponsor, "sponsor_bkg_number")
        member = {
            "number": number,
            "callsign": callsign,
            "name": row["name"],
            "qth": row["qth"],
            "join_date": row["qso_date"],
            "sponsor_bkg_number": sponsor,
            "sponsor_member": None,
            "og_regions": [],
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

    payload = {key: envelope[key] for key in ("schema_version", "members", "og_assignments")}
    canonical = json.dumps(payload, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    expected = "sha256:" + hashlib.sha256(canonical).hexdigest()
    _require(envelope["content_hash"] == expected, "Roster content hash mismatch")
    return envelope, members


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
                     environ=None) -> str:
    """Read an explicitly selected file or anonymous HTTPS roster endpoint.

    Call this only after selecting JSON input. With no file or explicit URL,
    ``ROSTER_EXPORT_URL`` is required. Network errors expose no URL, headers,
    credentials, response body, or automatic redirect/fallback in their message.
    """
    _require(not (path is not None and url is not None), "Select one roster JSON file or URL")
    if path is not None:
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
    # Public feeds need no authentication. Legacy Access endpoints may still
    # use a complete pair; never silently send just half of a service token.
    authenticated = any(environment.get(key) is not None for key, _ in auth_fields)
    for key, header in auth_fields if authenticated else ():
        credential = environment.get(key)
        _require(type(credential) is str and bool(credential.strip()) and
                 all(32 <= ord(char) <= 126 for char in credential),
                 "Access authentication requires a complete valid credential pair")
        headers[header] = credential
    request = urllib.request.Request(endpoint, headers=headers)
    try:
        with urllib.request.build_opener(_NoRedirects()).open(request, timeout=EXPORT_TIMEOUT_SECONDS) as response:
            _require(response.getcode() == 200, "Roster export did not return HTTP 200")
            return _decode_roster(response.read(MAX_ROSTER_BYTES + 1))
    except urllib.error.HTTPError as error:
        error.close()
        raise OSError(f"Unable to fetch roster JSON (HTTP {error.code})") from None
    except (urllib.error.URLError, OSError, http.client.HTTPException, UnicodeError):
        raise OSError("Unable to fetch roster JSON") from None
