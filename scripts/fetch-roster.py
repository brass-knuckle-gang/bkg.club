#!/usr/bin/env python3
"""Fetch and validate one roster snapshot before any workflow consumers run."""

import argparse
import json
from pathlib import Path
import sys
import tempfile

from roster_input import parse_roster_json, read_roster_json


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True,
                        help="private snapshot file outside the checkout and published output")
    parser.add_argument("--schema-version", choices=("1", "2"), default="1",
                        help="explicit public export contract (default: 1)")
    args = parser.parse_args(argv)
    try:
        destination = args.output.resolve()
        root = Path(__file__).resolve().parents[1]
        if destination.is_relative_to(root):
            raise ValueError("Snapshot must be outside the source repository and dist")
        text = read_roster_json()
        envelope, members = parse_roster_json(text, expected_version=args.schema_version)
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix=".roster-", dir=destination.parent) as temporary:
            staged = Path(temporary) / "snapshot.json"
            staged.write_text(text, encoding="utf-8")
            staged.replace(destination)
    except (ValueError, OSError):
        # The URL is a masking secret, not a credential. Keep errors generic
        # even outside Actions, where masking is unavailable.
        print("ERROR: Roster snapshot failed; check configuration, transport, and selected contract.",
              file=sys.stderr)
        return 1
    print(json.dumps({"active_members": len(members), "content_hash": envelope["content_hash"],
                      "generated_at": envelope["generated_at"], "schema_version": envelope["schema_version"]}, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
