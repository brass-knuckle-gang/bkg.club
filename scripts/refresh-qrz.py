#!/usr/bin/env python3
"""Explicitly refresh optional JSON-roster enrichment outside the repository.

This command never renders, deploys, or changes the production Sheets cache.
The JSON builder reads the sanitized result without contacting QRZ.
"""

import argparse
import contextlib
import http.client
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.parse
import urllib.request

from roster_enrichment import MAX_PHOTO_BYTES, read_photo_overrides, refresh_enrichment, validate_enrichment_dir
from roster_input import parse_roster_json, read_roster_json


def load_qrz_client():
    spec = importlib.util.spec_from_file_location("legacy_qrz_client", Path(__file__).with_name("build-roster.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    # Reuse the existing XML parsing only. The JSON refresh transport closes
    # failures, has a bounded response, and emits no raw authenticated URLs.
    module._fetch_with_retry = fetch_qrz
    return module


def _quiet(callback):
    # Legacy transport diagnostics can include request URLs or QRZ service
    # messages. Emit aggregate refresh counts only; never print raw responses.
    def invoke(*args, **kwargs):
        with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
            return callback(*args, **kwargs)
    return invoke


def reviewed_lookup(client, members):
    """QRZ cannot move reviewed identities or bind old geography to a new QTH.

    Geography is kept only when QRZ's country matches the reviewed one and,
    where both sides name a subdivision, the subdivision matches too. QRZ
    documents <state> as US-only even though it usually carries Canadian
    province codes, so a reviewed province is enforced only when QRZ reports
    one; a US state is always required.

    Lookups run on worker threads, so this does not swap sys.stderr itself;
    main() silences the whole refresh instead.
    """
    locations = {m["callsign"]: client.qth_location(m["qth"]) for m in members}

    def lookup(session, callsign):
        info = client.qrz_fetch_callsign(session, callsign)
        if not isinstance(info, dict):
            return info
        info = dict(info)
        state, country = locations[callsign]
        qrz_country = (info.get("country") or "").strip().casefold()
        if qrz_country in {"usa", "united states of america"}:
            qrz_country = "united states"
        qrz_state = (info.get("state") or "").strip().upper()
        subdivision_optional = qrz_country != "united states" and not qrz_state
        matches = (bool(state or country) and qrz_country == (country or "").casefold()
                   and (not state or subdivision_optional or qrz_state == state))
        if not matches:
            for key in ("grid", "lat", "lon"):
                info.pop(key, None)
        return info
    return lookup


class _PhotoRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        parsed = urllib.parse.urlsplit(newurl)
        if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username or parsed.password:
            raise ValueError("Unsafe photo redirect")
        return super().redirect_request(request, fp, code, msg, headers, newurl)


class _NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None


def fetch_qrz(request, *, timeout: float, what: str) -> bytes | None:
    """Bound authenticated XML downloads and never forward login redirects."""
    limit = 1024 * 1024
    try:
        with urllib.request.build_opener(_NoRedirectHandler()).open(request, timeout=timeout) as response:
            data = response.read(limit + 1)
            return data if len(data) <= limit else None
    except urllib.error.HTTPError as exc:
        exc.close()
        return None
    except (OSError, http.client.HTTPException):
        return None


def fetch_photo(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "BKG-JSON-Enrichment/1.0"})
    try:
        with urllib.request.build_opener(_PhotoRedirectHandler()).open(request, timeout=20) as response:
            return response.read(MAX_PHOTO_BYTES + 1)
    except urllib.error.HTTPError as exc:
        exc.close()
        raise ValueError("Photo download failed") from None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--roster-json", type=Path,
                        help="validated roster export file (otherwise anonymous ROSTER_EXPORT_URL)")
    parser.add_argument("--enrichment-dir", type=Path, required=True,
                        help="dedicated cache directory outside the source repository")
    parser.add_argument("--photo-overrides", type=Path,
                        help="explicit reviewed number/callsign manifest for custom photos")
    parser.add_argument("--overrides-only", action="store_true",
                        help="seed sanitized custom photos without QRZ (local preview only)")
    parser.add_argument("--require-usable", action="store_true",
                        help="fail bootstrap without both usable geography and photos")
    parser.add_argument("--max-age-hours", type=int,
                        help="look up only due members so each is rechecked within this many hours")
    parser.add_argument("--interval-hours", type=int, default=1,
                        help="how often this refresh runs; sizes each run's share (default: 1)")
    parser.add_argument("--workers", type=int, default=1,
                        help="concurrent QRZ lookups and photo downloads (default: 1)")
    parser.add_argument("--recheck", action="append", default=[], metavar="CALLSIGNS",
                        help="comma-separated reviewed callsigns to look up this run regardless of schedule")
    args = parser.parse_args(argv)
    recheck = frozenset(part.strip().upper() for value in args.recheck for part in value.split(",") if part.strip())
    try:
        directory = validate_enrichment_dir(args.enrichment_dir)
        if args.roster_json is not None and args.roster_json.resolve().is_relative_to(directory):
            raise ValueError("Roster input must be outside the enrichment cache directory")
        _envelope, members = parse_roster_json(read_roster_json(args.roster_json))
        unknown = sorted(recheck - {member["callsign"].upper() for member in members})
        if unknown:
            # Reviewed callsigns are public, so naming the typo is safe.
            print(f"ERROR: --recheck callsigns not in the roster: {', '.join(unknown)}", file=sys.stderr)
            return 1
        overrides = read_photo_overrides(args.photo_overrides, members) if args.photo_overrides else {}
        if args.overrides_only and not args.photo_overrides:
            raise ValueError("Overrides-only refresh requires a photo manifest")
        if not args.overrides_only and (not os.environ.get("QRZ_USERNAME") or not os.environ.get("QRZ_PASSWORD")):
            raise ValueError("QRZ_USERNAME and QRZ_PASSWORD are required for explicit refresh")
        if args.workers < 1 or (args.max_age_hours is not None and not 0 < args.interval_hours < args.max_age_hours):
            raise ValueError("Invalid refresh schedule")
        client = load_qrz_client()
        result = _quiet(refresh_enrichment)(
            members, directory, username="" if args.overrides_only else os.environ["QRZ_USERNAME"],
            password="" if args.overrides_only else os.environ["QRZ_PASSWORD"],
            login=client.qrz_login, lookup=reviewed_lookup(client, members), photo_fetch=fetch_photo,
            photo_overrides=overrides, require_usable=args.require_usable,
            max_age_seconds=None if args.max_age_hours is None else args.max_age_hours * 3600,
            interval_seconds=args.interval_hours * 3600, workers=args.workers, recheck=recheck,
        )
    except Exception:
        # Do not expose tokens, credentials, endpoint query strings, or raw
        # QRZ fields through exceptions from authenticated transport.
        print("ERROR: QRZ refresh failed; check the roster, external cache path, and credentials.", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
