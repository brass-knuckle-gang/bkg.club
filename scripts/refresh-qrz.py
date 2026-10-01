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

from roster_enrichment import MAX_PHOTO_BYTES, refresh_enrichment, validate_enrichment_dir
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
                        help="validated roster export file (otherwise authenticated ROSTER_EXPORT_URL)")
    parser.add_argument("--enrichment-dir", type=Path, required=True,
                        help="dedicated cache directory outside the source repository")
    args = parser.parse_args(argv)
    try:
        directory = validate_enrichment_dir(args.enrichment_dir)
        if args.roster_json is not None and args.roster_json.resolve().is_relative_to(directory):
            raise ValueError("Roster input must be outside the enrichment cache directory")
        _envelope, members = parse_roster_json(read_roster_json(args.roster_json))
        if not os.environ.get("QRZ_USERNAME") or not os.environ.get("QRZ_PASSWORD"):
            raise ValueError("QRZ_USERNAME and QRZ_PASSWORD are required for explicit refresh")
        client = load_qrz_client()
        result = refresh_enrichment(
            members, directory, username=os.environ["QRZ_USERNAME"], password=os.environ["QRZ_PASSWORD"],
            login=_quiet(client.qrz_login), lookup=_quiet(client.qrz_fetch_callsign), photo_fetch=fetch_photo,
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
