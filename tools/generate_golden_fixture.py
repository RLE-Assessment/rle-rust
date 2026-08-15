#!/usr/bin/env python3
"""Generate the null-island AOO fixture from rle-python's committed golden data.

`rle-python` ships a byte-level snapshot of its AOO grid for a small synthetic
dataset, and asserts against it on every run. Porting it here is what makes
"the Rust engine agrees with the Python one" a checkable claim rather than an
aspiration.

Two files are read, both from the rle-python checkout:

  tests/test_data/null_island.geojson              the input distribution
  tests/test_data/aoo_grid_null_island_golden.parquet   its expected AOO grid

Neither is modified. The output records the input geometry alongside the expected
per-cell fractions, so the fixture is self-contained and the Rust tests need neither
Python nor a sibling checkout.

Usage:
    python3 tools/generate_golden_fixture.py [--rle-python PATH]
    python3 tools/generate_golden_fixture.py --check
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import pyarrow.parquet as pq

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_RLE_PYTHON = ROOT.parent / "rle-python"
OUTPUT = ROOT / "fixtures" / "cases" / "null_island_aoo.json"

# rle-python names its fraction columns by slugifying the ecosystem NAME, replacing
# every non-alphanumeric character with an underscore. Mapping back to the ECO_CODE
# keeps this fixture keyed on the stable identifier rather than the display name.
def slugify(name: str) -> str:
    return "".join(c if c.isalnum() or c in "_-" else "_" for c in name)


def build(rle_python: Path) -> dict:
    geojson_path = rle_python / "tests" / "test_data" / "null_island.geojson"
    parquet_path = (
        rle_python / "tests" / "test_data" / "aoo_grid_null_island_golden.parquet"
    )

    for path in (geojson_path, parquet_path):
        if not path.exists():
            raise SystemExit(f"error: {path} not found; pass --rle-python PATH")

    geojson = json.loads(geojson_path.read_text())

    features = []
    for feature in geojson["features"]:
        properties = feature["properties"]
        geometry = feature["geometry"]
        if geometry["type"] != "Polygon":
            raise SystemExit(f"unexpected geometry type {geometry['type']}")
        features.append(
            {
                "code": properties["ECO_CODE"],
                "name": properties["ECO_NAME"],
                # GeoJSON: ring 0 is the exterior, the rest are holes.
                "rings": geometry["coordinates"],
            }
        )

    table = pq.read_table(parquet_path).to_pandas()
    by_slug = {slugify(f["name"]): f["code"] for f in features}

    expected = []
    for _, row in table.iterrows():
        fractions = {}
        for slug, code in by_slug.items():
            if slug in table.columns:
                fractions[code] = float(row[slug])
        expected.append(
            {
                "grid_col": int(row["grid_col"]),
                "grid_row": int(row["grid_row"]),
                "count_geoms": int(row["count_geoms"]),
                "count_ecosystems": int(row["count_ecosystems"]),
                "fractions": fractions,
            }
        )

    expected.sort(key=lambda c: (c["grid_col"], c["grid_row"]))

    return {
        "schema_version": 1,
        "function": "aoo_grid",
        "source": "rle-python tests/test_data/aoo_grid_null_island_golden.parquet",
        "description": [
            "The AOO grid rle-python computes for its null-island test dataset,",
            "ported so the Rust engine can be checked against the implementation",
            "that assessments currently use.",
            "",
            "CONFORMANCE CONTRACT. Discrete outputs must match EXACTLY: the set of",
            "occupied (grid_col, grid_row) cells, and which ecosystems occupy each.",
            "Continuous outputs — the per-cell fractions — are compared with a",
            "relative tolerance, because the two implementations compute them",
            "differently and bit-identical floats are not achievable:",
            "",
            "  * rle-python clips with GEOS; this engine uses Sutherland-Hodgman",
            "    against the cell rectangle.",
            "  * rle-python builds its grid in ESRI:54034, converts it to EPSG:4326,",
            "    and converts it back before intersecting, so its cells are slightly",
            "    distorted four-corner approximations. This engine clips against exact",
            "    rectangles.",
            "",
            "The measured divergence is recorded in the Rust test. What matters is",
            "that no decision built on these floats — cell membership, the 1%",
            "exclusion, the AOO count, the category — differs.",
        ],
        "features": features,
        "expected_cells": expected,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rle-python", type=Path, default=DEFAULT_RLE_PYTHON)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    generated = json.dumps(build(args.rle_python), indent=2) + "\n"

    if args.check:
        current = OUTPUT.read_text() if OUTPUT.exists() else ""
        if current != generated:
            print(f"error: {OUTPUT.relative_to(ROOT)} is out of date", file=sys.stderr)
            return 1
        print(f"{OUTPUT.relative_to(ROOT)} is up to date")
        return 0

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(generated)
    print(f"wrote {OUTPUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
