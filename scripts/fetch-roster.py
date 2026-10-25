#!/usr/bin/env python3
"""Fetch and validate one roster snapshot before any workflow consumers run."""

import argparse
import json
import os
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
    parser.add_argument("--cache-dir", type=Path,
                        help="optional version-specific conditional GET cache outside the checkout")
    args = parser.parse_args(argv)
    try:
        destination = args.output.resolve()
        root = Path(__file__).resolve().parents[1]
        if destination.is_relative_to(root):
            raise ValueError("Snapshot must be outside the source repository and dist")
        if args.cache_dir:
            cache = args.cache_dir.resolve()
            if cache.is_relative_to(root) or root.is_relative_to(cache) or destination.is_relative_to(cache):
                raise ValueError("Roster cache must be outside the checkout and separate from the snapshot")
        source = destination.with_name(destination.stem + "-source" + destination.suffix)
        text = read_roster_json(expected_version=args.schema_version, cache_dir=args.cache_dir,
                                exclude_invalid_callsigns=True)
        exclusions = []
        envelope, members = parse_roster_json(text, expected_version=args.schema_version,
                                               exclude_invalid_callsigns=True, exclusions=exclusions)
        original = json.loads(text)
        publication = json.dumps(envelope, ensure_ascii=False, indent=2) + "\n" if exclusions else text
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix=".roster-", dir=destination.parent) as temporary:
            staged = Path(temporary) / "snapshot.json"
            staged.write_text(publication, encoding="utf-8")
            saved_source = Path(temporary) / "source.json"
            saved_source.write_text(text, encoding="utf-8")
            saved_source.replace(source)
            staged.replace(destination)
    except (ValueError, OSError):
        # The URL is a masking secret, not a credential. Keep errors generic
        # even outside Actions, where masking is unavailable.
        print("ERROR: Roster snapshot failed; check configuration, transport, and selected contract.",
              file=sys.stderr)
        return 1
    for exclusion in exclusions:
        print(f"::warning title=Roster member excluded::BKG #{exclusion['bkg_number']}: {exclusion['reason']}",
              file=sys.stderr)
    summary = (f"Roster publication: {len(members)} accepted, {len(exclusions)} excluded "
               f"from {len(original['members'])} source members.\n")
    summary += "".join(f"- BKG #{row['bkg_number']}: {row['reason']}\n" for row in exclusions)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as report:
            report.write(summary)
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(f"excluded_members={len(exclusions)}\n")
    print(json.dumps({"active_members": len(members), "source_members": len(original["members"]),
                      "source_content_hash": original["content_hash"], "exclusions": exclusions,
                      "content_hash": envelope["content_hash"],
                      "generated_at": envelope["generated_at"], "schema_version": envelope["schema_version"]}, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
