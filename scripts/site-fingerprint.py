#!/usr/bin/env python3
"""Print a digest of a built site that ignores build timestamps.

The scheduled deploy compares this with the last deployed digest and skips
uploading when nothing a visitor could see has changed. Anything not
normalized here counts as a change, so an unknown difference deploys.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys


def normalized(relative: str, data: bytes) -> bytes:
    if relative == "index.html":
        return re.sub(rb"(<!-- BUILD_TIME:START -->).*?(<!-- BUILD_TIME:END -->)", rb"\1\2", data)
    if relative == "members.txt":
        return re.sub(rb"(?m)^# Generated: .*\n", b"", data, count=1)
    if relative == "data/v1/roster.json":
        # The export stamps every response; content_hash covers the members.
        envelope = json.loads(data)
        envelope.pop("generated_at", None)
        return json.dumps(envelope, sort_keys=True).encode()
    return data


def fingerprint(output_dir: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(p for p in output_dir.rglob("*") if p.is_file()):
        relative = path.relative_to(output_dir).as_posix()
        content = hashlib.sha256(normalized(relative, path.read_bytes())).hexdigest()
        digest.update(f"{relative}\0{content}\n".encode())
    return digest.hexdigest()


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_dir", type=Path)
    args = parser.parse_args(argv)
    print(fingerprint(args.output_dir))
    return 0


if __name__ == "__main__":
    sys.exit(main())
