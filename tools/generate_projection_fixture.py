#!/usr/bin/env python3
"""Generate the ESRI:54034 projection reference fixture from PROJ.

The AOO grid is defined in ESRI:54034 (World Cylindrical Equal Area), so every
occupied-cell count depends on getting this projection right. PROJ is the reference
implementation the rest of the geospatial world agrees with, including `rle-python`
via pyproj, so PROJ's answers are the ones to match.

Committing the generated values means the Rust tests need no PROJ at runtime, and the
same fixture can be run through every language binding.

Usage:
    python3 tools/generate_projection_fixture.py            # write the fixture
    python3 tools/generate_projection_fixture.py --check    # fail if out of date

Requires pyproj.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import pyproj

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "fixtures" / "cases" / "projection.json"

# Chosen to exercise the places a projection implementation goes wrong: the origin,
# both antimeridians, both poles, both hemispheres in each axis, and the real
# assessment areas this library was built for.
POINTS: list[tuple[float, float, str]] = [
    (0.0, 0.0, "the projection origin"),
    (1.0, 1.0, "a short way from the origin"),
    (-1.0, -1.0, "the opposite quadrant, checking sign handling"),
    (180.0, 0.0, "the eastern antimeridian"),
    (-180.0, 0.0, "the western antimeridian"),
    (0.0, 90.0, "the north pole, where the authalic term saturates"),
    (0.0, -90.0, "the south pole"),
    (0.0, 89.9, "just short of the north pole"),
    (45.0, 45.0, "mid-latitude, both positive"),
    (-45.0, -45.0, "mid-latitude, both negative"),
    (-73.5, 4.2, "Bogota, Colombia"),
    (-75.5, 6.25, "Medellin, Colombia"),
    (-70.0, -4.2, "southern Colombia, across the equator"),
    (25.5, -33.7, "Great Fish Thicket, South Africa (Guidelines Box 12)"),
    (18.5, -34.0, "Cape Flats Sand Fynbos, South Africa (Guidelines Box 14)"),
    (145.5, -30.0, "Coolibah-Black Box Woodland, Australia (Box 14)"),
    (-159.0, 22.0, "mid-Pacific, far from the central meridian"),
    (100.0, 60.0, "high northern latitude"),
    (-60.0, -60.0, "high southern latitude"),
    (0.0, 0.5, "a fraction of a degree north, near the equator"),
]


def build() -> dict:
    transformer = pyproj.Transformer.from_crs("EPSG:4326", "ESRI:54034", always_xy=True)

    cases = []
    for lon, lat, why in POINTS:
        x, y = transformer.transform(lon, lat)
        cases.append(
            {
                "why": why,
                "lon": lon,
                "lat": lat,
                # repr() keeps full float64 precision through JSON.
                "x": float(repr(x)),
                "y": float(repr(y)),
            }
        )

    return {
        "schema_version": 1,
        "function": "project_to_aoo_crs",
        "source_crs": "EPSG:4326",
        "target_crs": "ESRI:54034",
        "generated_by": f"pyproj {pyproj.__version__} / PROJ {pyproj.proj_version_str}",
        "description": [
            "Reference coordinates for the ESRI:54034 World Cylindrical Equal Area",
            "projection, generated from PROJ.",
            "",
            "The AOO grid is defined in this projection, so every occupied-cell count",
            "depends on it. PROJ is what the rest of the geospatial world agrees with,",
            "including rle-python via pyproj, so these are the values to match.",
            "",
            "Tolerance is 1e-6 m (one micrometre). The forward transform is closed-form",
            "for a standard parallel of 0, with no iteration and no series truncation,",
            "so agreement is limited only by floating-point rounding — measured at under",
            "2 nanometres when this fixture was written.",
        ],
        "tolerance_m": 1e-6,
        "cases": cases,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="verify the fixture is up to date"
    )
    args = parser.parse_args()

    generated = json.dumps(build(), indent=2) + "\n"

    if args.check:
        current = OUTPUT.read_text() if OUTPUT.exists() else ""
        if current != generated:
            print(
                f"error: {OUTPUT.relative_to(ROOT)} is out of date.\n"
                f"       Run: python3 tools/generate_projection_fixture.py",
                file=sys.stderr,
            )
            return 1
        print(f"{OUTPUT.relative_to(ROOT)} is up to date")
        return 0

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(generated)
    print(f"wrote {OUTPUT.relative_to(ROOT)} ({len(POINTS)} reference points)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
