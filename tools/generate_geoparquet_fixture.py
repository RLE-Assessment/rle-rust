#!/usr/bin/env python3
"""Generate the GeoParquet reading fixture with geopandas.

The Rust reader has to work on files written by the tools assessors actually use, and
geopandas is the overwhelmingly common one. Writing the fixture with geopandas rather
than assembling parquet bytes by hand means the tests check agreement with a real
writer's choices — how it spells the `geo` metadata, how it lays out the GeoParquet 1.1
bbox covering struct, which statistics it emits — instead of agreeing with whatever the
Rust side happens to assume.

The file is small (a few kilobytes) and committed, so Rust tests and CI need no Python.

Layout, chosen so row-group pruning has something real to prune:

    row group 0   ECO_A   four polygons near lon -80
    row group 1   ECO_B   four polygons near lon -60
    row group 2   ECO_C   four polygons near lon   0
    row group 3   ECO_D   four polygons near lon  40

Each row group holds exactly one ecosystem in one longitude band, so a bbox filter and
an ecosystem filter each select a known, checkable subset. A reader that ignores
statistics still returns correct answers — just slowly — so the tests assert on the
*plan* (which row groups were selected) as well as the decoded results.

Usage:
    python3 tools/generate_geoparquet_fixture.py            # write the fixture
    python3 tools/generate_geoparquet_fixture.py --check    # fail if out of date

Requires geopandas and pyarrow. In this repo:
    pixi run --manifest-path ../rle-python/pyproject.toml python tools/generate_geoparquet_fixture.py
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import geopandas as gpd
import pyarrow.parquet as pq
from shapely.geometry import MultiPolygon, Polygon

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "fixtures" / "data" / "ecosystems.parquet"
# The same data written without the GeoParquet 1.1 bbox covering columns, which is what
# every 1.0 file looks like. Spatial pruning is impossible on these, and the reader must
# fall back to reading everything rather than quietly returning nothing.
DATA_NO_COVERING = ROOT / "fixtures" / "data" / "ecosystems_no_covering.parquet"
MANIFEST = ROOT / "fixtures" / "cases" / "geoparquet.json"

ROW_GROUP_SIZE = 4

# (ecosystem code, longitude band centre). One band per ecosystem, well separated so a
# bbox filter selects an unambiguous set of row groups.
BANDS = [("ECO_A", -80.0), ("ECO_B", -60.0), ("ECO_C", 0.0), ("ECO_D", 40.0)]


def square(lon: float, lat: float, size: float) -> Polygon:
    """An axis-aligned square with its lower-left corner at (lon, lat)."""
    return Polygon(
        [
            (lon, lat),
            (lon + size, lat),
            (lon + size, lat + size),
            (lon, lat + size),
            (lon, lat),
        ]
    )


def build() -> gpd.GeoDataFrame:
    records = []
    for code, centre in BANDS:
        for index in range(ROW_GROUP_SIZE):
            lon = centre + index * 2.0
            lat = float(index)
            if index == ROW_GROUP_SIZE - 1:
                # One MultiPolygon per ecosystem, with a hole in the first part. Both
                # are places a reader can quietly go wrong: flattening the parts, or
                # treating the hole as another patch.
                shell = square(lon, lat, 1.0)
                holed = Polygon(
                    shell.exterior.coords,
                    [square(lon + 0.25, lat + 0.25, 0.5).exterior.coords],
                )
                geometry = MultiPolygon([holed, square(lon + 3.0, lat, 0.5)])
            else:
                geometry = square(lon, lat, 1.0)
            records.append(
                {
                    "eco_code": code,
                    "feature_id": f"{code}-{index}",
                    "geometry": geometry,
                }
            )

    return gpd.GeoDataFrame(records, crs="EPSG:4326")


def manifest_for(frame: gpd.GeoDataFrame, path: Path) -> dict:
    """Describe the written file so the Rust tests assert against measured truth.

    Everything here is read back out of the file rather than assumed, so if geopandas
    changes how it writes, `--check` reports it instead of the Rust tests failing with
    no explanation.
    """
    parquet = pq.ParquetFile(path)
    geo = json.loads(parquet.schema_arrow.metadata[b"geo"])
    primary = geo["primary_column"]

    row_groups = []
    for index in range(parquet.metadata.num_row_groups):
        group = parquet.metadata.row_group(index)
        codes = sorted(
            {
                frame.iloc[row]["eco_code"]
                for row in range(
                    index * ROW_GROUP_SIZE,
                    min((index + 1) * ROW_GROUP_SIZE, len(frame)),
                )
            }
        )
        bounds = frame.iloc[
            index * ROW_GROUP_SIZE : (index + 1) * ROW_GROUP_SIZE
        ].total_bounds
        row_groups.append(
            {
                "index": index,
                "num_rows": group.num_rows,
                "eco_codes": codes,
                "bounds": [round(value, 10) for value in bounds],
            }
        )

    return {
        "description": (
            "A GeoParquet 1.1 file written by geopandas, used to check remote reading: "
            "footer parsing, `geo` metadata, row-group pruning by bounding box and by "
            "ecosystem code, and WKB decoding of polygons and multipolygons."
        ),
        "generated_by": f"geopandas {gpd.__version__}",
        "path": str(path.relative_to(ROOT)),
        "file_size": path.stat().st_size,
        "geo_version": geo["version"],
        "primary_column": primary,
        "encoding": geo["columns"][primary]["encoding"],
        "crs_id": geo["columns"][primary]["crs"]["id"],
        "covering_bbox_column": geo["columns"][primary]
        .get("covering", {})
        .get("bbox", {})
        .get("xmin", [None])[0],
        "num_rows": parquet.metadata.num_rows,
        "num_row_groups": parquet.metadata.num_row_groups,
        "row_groups": row_groups,
        "total_bounds": [round(value, 10) for value in frame.total_bounds],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="fail if the fixture is out of date"
    )
    args = parser.parse_args()

    frame = build()
    target = DATA if not args.check else DATA.with_suffix(".check.parquet")
    target.parent.mkdir(parents=True, exist_ok=True)
    MANIFEST.parent.mkdir(parents=True, exist_ok=True)

    frame.to_parquet(
        target,
        row_group_size=ROW_GROUP_SIZE,
        write_covering_bbox=True,
        # Uncompressed so a byte range maps directly to readable data, which keeps the
        # Rust tests debuggable, and so the fixture does not change if a compression
        # library is upgraded.
        compression=None,
    )
    plain = (
        DATA_NO_COVERING
        if not args.check
        else DATA_NO_COVERING.with_suffix(".check.parquet")
    )
    frame.to_parquet(
        plain,
        row_group_size=ROW_GROUP_SIZE,
        write_covering_bbox=False,
        compression=None,
    )

    manifest = manifest_for(frame, target)
    manifest["path"] = str(DATA.relative_to(ROOT))
    manifest["file_size"] = target.stat().st_size
    manifest["path_without_covering"] = str(DATA_NO_COVERING.relative_to(ROOT))

    if args.check:
        target.unlink()
        plain.unlink()
        if not MANIFEST.exists():
            print(f"{MANIFEST} does not exist; run without --check", file=sys.stderr)
            return 1
        current = json.loads(MANIFEST.read_text())
        # The writer version is expected to drift and says nothing about correctness.
        for record in (manifest, current):
            record.pop("generated_by", None)
            record.pop("file_size", None)
        if current != manifest:
            print(
                f"{MANIFEST} is out of date; re-run without --check", file=sys.stderr
            )
            return 1
        print(f"{MANIFEST.relative_to(ROOT)} is up to date")
        return 0

    MANIFEST.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"wrote {DATA.relative_to(ROOT)} ({manifest['file_size']} bytes)")
    print(f"wrote {MANIFEST.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
