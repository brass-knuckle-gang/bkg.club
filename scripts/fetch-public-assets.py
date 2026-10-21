#!/usr/bin/env python3
"""Save immutable v2 photos from the saved administration roster snapshot."""

import argparse
import json
import os
from pathlib import Path
import sys

from roster_assets import fetch_assets
from roster_input import parse_roster_json, read_roster_json


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--roster-json", type=Path, required=True)
    parser.add_argument("--asset-dir", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        if args.roster_json.resolve().is_relative_to(args.asset_dir.resolve()):
            raise ValueError("Snapshot must be outside the asset directory")
        _envelope, members = parse_roster_json(read_roster_json(args.roster_json), expected_version="2")
        result = fetch_assets(members, args.asset_dir, source_url=os.environ.get("ROSTER_EXPORT_URL"))
    except Exception:
        print("ERROR: Public asset publication failed; check the v2 snapshot and asset origin/bytes.", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
