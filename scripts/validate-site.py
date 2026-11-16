#!/usr/bin/env python3
"""Reject incomplete public builds before uploading them to GitHub Pages."""

import argparse
import sys
from pathlib import Path

from site_contract import validate_dist


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", nargs="?", type=Path, default=Path("dist"))
    args = parser.parse_args()
    try:
        validate_dist(args.output_dir)
    except (ValueError, OSError, KeyError, TypeError) as exc:
        print(f"ERROR: Public build validation failed: {exc}", file=sys.stderr)
        return 1
    print(f"Validated public build: {args.output_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
