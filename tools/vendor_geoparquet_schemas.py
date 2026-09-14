#!/usr/bin/env python3
"""Vendor the published GeoParquet JSON Schemas, and whatever they reference.

Validation has to work offline: in CI without network, and in a browser where fetching
a schema mid-parse is not an option. So the schemas are committed rather than
downloaded at run time, and this script is how they get here.

The external references matter more than they look. Each release's `crs` field is a
`$ref` to PROJJSON on proj.org — 38 KB and 53 definitions — and it is genuinely
enforced: a validator that cannot resolve it either fails outright or silently skips
the subschema, and the second is worse. So references are followed and vendored too,
discovered by scanning rather than hard-coded, since they differ between releases.

Usage:
    python3 tools/vendor_geoparquet_schemas.py            # fetch and write
    python3 tools/vendor_geoparquet_schemas.py --check    # fail if out of date

Needs network access. The committed result does not.
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCHEMAS = ROOT / "crates" / "iucn-rle-format" / "schemas"
INDEX = SCHEMAS / "index.json"

RELEASES_URL = "https://geoparquet.org/releases"

# Every release published at geoparquet.org/releases. Pre-releases are included
# because files declaring them exist in the wild, and a validator that cannot find a
# schema for a declared version has nothing useful to say about the file.
VERSIONS = [
    "v0.2.0",
    "v0.3.0",
    "v0.4.0",
    "v1.0.0-beta.1",
    "v1.0.0-rc.1",
    "v1.0.0",
    "v1.1.0",
    "v2.0.0-rc.1",
]


def fetch(url: str) -> bytes:
    # An explicit User-Agent is required, not politeness: proj.org answers urllib's
    # default with 403.
    request = urllib.request.Request(
        url, headers={"User-Agent": "rle-rust schema vendoring (github.com/RLE-Assessment)"}
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def external_refs(node: object) -> set[str]:
    """Every absolute `$ref` under a schema."""
    found: set[str] = set()
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "$ref" and isinstance(value, str) and value.startswith("http"):
                found.add(value)
            else:
                found |= external_refs(value)
    elif isinstance(node, list):
        for item in node:
            found |= external_refs(item)
    return found


def filename_for(url: str) -> str:
    """A flat, stable filename for a referenced schema."""
    return url.split("://", 1)[1].replace("/", "_")


def collect() -> dict[str, bytes]:
    """Every schema to vendor, keyed by the filename it will be written as."""
    files: dict[str, bytes] = {}
    referenced: set[str] = set()

    for version in VERSIONS:
        raw = fetch(f"{RELEASES_URL}/{version}/schema.json")
        files[f"{version}.json"] = raw
        referenced |= external_refs(json.loads(raw))

    # Follow references transitively; PROJJSON refers only to itself, but nothing here
    # depends on that staying true.
    seen: set[str] = set()
    while referenced - seen:
        url = (referenced - seen).pop()
        seen.add(url)
        raw = fetch(url)
        files[filename_for(url)] = raw
        referenced |= external_refs(json.loads(raw))

    return files


def index_for(files: dict[str, bytes], refs: dict[str, str]) -> dict:
    return {
        "description": (
            "Published GeoParquet JSON Schemas and the external schemas they "
            "reference, vendored so validation works offline and in the browser."
        ),
        "source": RELEASES_URL,
        "versions": {version: f"{version}.json" for version in VERSIONS},
        "references": refs,
        "sizes": {name: len(raw) for name, raw in sorted(files.items())},
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="fail if the vendored copies are stale"
    )
    args = parser.parse_args()

    files = collect()
    refs = {
        url: filename_for(url)
        for version in VERSIONS
        for url in external_refs(json.loads(files[f"{version}.json"]))
    }
    index = index_for(files, refs)

    if args.check:
        if not INDEX.exists():
            print(f"{INDEX} does not exist; run without --check", file=sys.stderr)
            return 1
        current = json.loads(INDEX.read_text())
        if current != index:
            print(f"{INDEX} is out of date; re-run without --check", file=sys.stderr)
            return 1
        for name, raw in files.items():
            path = SCHEMAS / name
            if not path.exists() or path.read_bytes() != raw:
                print(f"{path} differs from upstream; re-run without --check", file=sys.stderr)
                return 1
        print(f"{SCHEMAS.relative_to(ROOT)} matches upstream")
        return 0

    SCHEMAS.mkdir(parents=True, exist_ok=True)
    for name, raw in files.items():
        (SCHEMAS / name).write_bytes(raw)
    INDEX.write_text(json.dumps(index, indent=2) + "\n")

    total = sum(len(raw) for raw in files.values())
    print(f"vendored {len(files)} schemas ({total / 1024:.0f} KB) into {SCHEMAS.relative_to(ROOT)}")
    for url, name in refs.items():
        print(f"  reference: {url} -> {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
