#!/usr/bin/env python3
"""Generate Cloud-Optimized GeoTIFF fixtures with GDAL.

The Rust reader has to work on files produced by the tool that defined the format, so
the fixtures are written by GDAL's own COG driver rather than assembled by hand. That
makes the tests a check on agreement with the reference implementation: how the IFDs are
ordered, where the tile offsets land, what the overviews look like.

Deliberately written in ESRI:54034 — the projection the AOO grid is defined in, and the
one `@developmentseed/geotiff` rejects with *"Unsupported coordinate transformation type:
28"*. Reading it is the browser capability this project exists to provide.

Two variants are produced, because compression is the likeliest place a reader breaks:

    ecosystems_cog.tif             DEFLATE, what real COGs use
    ecosystems_cog_uncompressed.tif  no compression, to isolate decode failures

The raster holds ecosystem class codes, one byte per pixel, on a grid whose pixels are
exactly the 10 km AOO cells. Pixel value is `(x // 64) + 8 * (y // 64)`, which varies
both within and across tiles — a reader that mixes up tile order, or reads an overview
in place of full resolution, gets visibly wrong numbers rather than plausible ones.

Usage:
    python3 tools/generate_cog_fixture.py            # write the fixtures
    python3 tools/generate_cog_fixture.py --check    # fail if out of date

Requires the GDAL command-line tools (gdal_translate, gdalinfo) on PATH, or set
GDAL_BIN to the directory holding them.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "fixtures" / "data"
MANIFEST = ROOT / "fixtures" / "cases" / "cog.json"

SIZE = 512
BLOCK = 256
# One pixel is one AOO cell, and the origin is the projection origin, so a pixel's
# column and row are its grid cell's column and row. Nothing has to be recomputed to
# check an answer by hand.
PIXEL_METRES = 10_000.0
CRS = "ESRI:54034"


def gdal_tool(name: str) -> str:
    directory = os.environ.get("GDAL_BIN")
    candidate = str(Path(directory) / name) if directory else shutil.which(name)
    if not candidate or not Path(candidate).exists():
        raise SystemExit(
            f"{name} not found. Install GDAL or set GDAL_BIN to the directory "
            f"containing it."
        )
    return candidate


def ecosystem_code(x: int, y: int) -> int:
    """The class code at a pixel. Varies within a tile and across tiles."""
    return (x // 64) + 8 * (y // 64)


def write_raw(path: Path) -> None:
    rows = bytearray()
    for y in range(SIZE):
        rows.extend(ecosystem_code(x, y) for x in range(SIZE))
    path.write_bytes(bytes(rows))


def write_vrt_geographic(path: Path, raw_name: str) -> None:
    """The same pixels in EPSG:4326, which the AOO grid must refuse.

    Pixel edges are curved in the grid's equal-area plane, so every pixel would need
    approximating. A reader that accepted this would produce numbers that look fine and
    are wrong, which is worse than an error.
    """
    path.write_text(
        f"""<VRTDataset rasterXSize="{SIZE}" rasterYSize="{SIZE}">
  <SRS>EPSG:4326</SRS>
  <GeoTransform>-10.0, 0.02, 0.0, 10.0, 0.0, -0.02</GeoTransform>
  <VRTRasterBand dataType="Byte" band="1" subClass="VRTRawRasterBand">
    <SourceFilename relativeToVRT="1">{raw_name}</SourceFilename>
    <ImageOffset>0</ImageOffset>
    <PixelOffset>1</PixelOffset>
    <LineOffset>{SIZE}</LineOffset>
  </VRTRasterBand>
</VRTDataset>
"""
    )


def write_vrt(path: Path, raw_name: str) -> None:
    # A raw VRT lets GDAL read a flat byte array with no raster library on this side.
    path.write_text(
        f"""<VRTDataset rasterXSize="{SIZE}" rasterYSize="{SIZE}">
  <SRS>{CRS}</SRS>
  <GeoTransform>0.0, {PIXEL_METRES}, 0.0, 0.0, 0.0, -{PIXEL_METRES}</GeoTransform>
  <VRTRasterBand dataType="Byte" band="1" subClass="VRTRawRasterBand">
    <SourceFilename relativeToVRT="1">{raw_name}</SourceFilename>
    <ImageOffset>0</ImageOffset>
    <PixelOffset>1</PixelOffset>
    <LineOffset>{SIZE}</LineOffset>
  </VRTRasterBand>
</VRTDataset>
"""
    )


def translate(vrt: Path, target: Path, compression: str) -> None:
    subprocess.run(
        [
            gdal_tool("gdal_translate"),
            "-of",
            "COG",
            "-co",
            f"BLOCKSIZE={BLOCK}",
            "-co",
            f"COMPRESS={compression}",
            # Overviews are what make a COG a COG, and they add IFDs after the first.
            # Keeping them means the fixture catches a reader that picks the wrong one.
            "-co",
            "OVERVIEWS=AUTO",
            str(vrt),
            str(target),
        ],
        check=True,
        capture_output=True,
    )


def tiff_layout(path: Path) -> dict:
    """Read the TIFF header directly to record what the Rust reader must find.

    Parsed here rather than taken from gdalinfo because these are exactly the fields a
    byte-range reader depends on, and recording them measured — not assumed — is what
    makes the Rust tests meaningful.
    """
    data = path.read_bytes()
    byte_order = data[:2]
    little = byte_order == b"II"
    prefix = "<" if little else ">"
    magic = struct.unpack_from(f"{prefix}H", data, 2)[0]
    if magic != 42:
        raise SystemExit(f"{path} is BigTIFF or malformed (magic {magic})")

    ifd_offsets = []
    offset = struct.unpack_from(f"{prefix}I", data, 4)[0]
    first_ifd = offset
    while offset:
        ifd_offsets.append(offset)
        count = struct.unpack_from(f"{prefix}H", data, offset)[0]
        offset = struct.unpack_from(f"{prefix}I", data, offset + 2 + count * 12)[0]

    return {
        "byte_order": "little" if little else "big",
        "first_ifd_offset": first_ifd,
        "ifd_count": len(ifd_offsets),
        "ifd_offsets": ifd_offsets,
    }


def manifest_for(paths: dict[str, Path]) -> dict:
    info = json.loads(
        subprocess.run(
            [gdal_tool("gdalinfo"), "-json", str(paths["deflate"])],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    band = info["bands"][0]

    # Values a windowed read must return, at points chosen to sit in different tiles.
    probes = [
        {"x": 0, "y": 0, "value": ecosystem_code(0, 0)},
        {"x": 255, "y": 255, "value": ecosystem_code(255, 255)},
        {"x": 256, "y": 0, "value": ecosystem_code(256, 0)},
        {"x": 0, "y": 256, "value": ecosystem_code(0, 256)},
        {"x": 511, "y": 511, "value": ecosystem_code(511, 511)},
        {"x": 300, "y": 100, "value": ecosystem_code(300, 100)},
    ]

    return {
        "description": (
            "Cloud-Optimized GeoTIFFs written by GDAL's COG driver, in ESRI:54034 with "
            "10 km pixels, used to check windowed raster reading over byte ranges."
        ),
        "generated_by": f"GDAL {info['metadata'].get('', {}).get('', '')}".strip()
        or "GDAL",
        "width": SIZE,
        "height": SIZE,
        "block_size": BLOCK,
        "pixel_metres": PIXEL_METRES,
        "crs": CRS,
        "data_type": band["type"],
        "geotransform": info["geoTransform"],
        "tiles_across": SIZE // BLOCK,
        "tiles_down": SIZE // BLOCK,
        "probes": probes,
        "files": {
            name: {
                # --check builds into a temporary directory, so the path is only
                # repo-relative when generating for real. It is dropped before
                # comparison either way.
                "path": str(path.relative_to(ROOT))
                if path.is_relative_to(ROOT)
                else path.name,
                "layout": tiff_layout(path),
            }
            for name, path in paths.items()
        },
    }


def build(target_dir: Path) -> dict[str, Path]:
    with tempfile.TemporaryDirectory() as scratch:
        scratch_path = Path(scratch)
        raw = scratch_path / "ecosystems.raw"
        vrt = scratch_path / "ecosystems.vrt"
        geographic = scratch_path / "ecosystems_geographic.vrt"
        write_raw(raw)
        write_vrt(vrt, raw.name)
        write_vrt_geographic(geographic, raw.name)

        paths = {
            "deflate": target_dir / "ecosystems_cog.tif",
            "uncompressed": target_dir / "ecosystems_cog_uncompressed.tif",
            "geographic": target_dir / "ecosystems_cog_wgs84.tif",
        }
        translate(vrt, paths["deflate"], "DEFLATE")
        translate(vrt, paths["uncompressed"], "NONE")
        translate(geographic, paths["geographic"], "DEFLATE")
        return paths


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="fail if the fixtures are out of date"
    )
    args = parser.parse_args()

    DATA.mkdir(parents=True, exist_ok=True)
    MANIFEST.parent.mkdir(parents=True, exist_ok=True)

    if args.check:
        with tempfile.TemporaryDirectory() as scratch:
            paths = build(Path(scratch))
            fresh = manifest_for(paths)
        if not MANIFEST.exists():
            print(f"{MANIFEST} does not exist; run without --check", file=sys.stderr)
            return 1
        current = json.loads(MANIFEST.read_text())
        for record in (fresh, current):
            record.pop("generated_by", None)
            # Paths differ because the check builds into a temporary directory.
            for entry in record.get("files", {}).values():
                entry.pop("path", None)
        if fresh != current:
            print(f"{MANIFEST} is out of date; re-run without --check", file=sys.stderr)
            return 1
        print(f"{MANIFEST.relative_to(ROOT)} is up to date")
        return 0

    paths = build(DATA)
    MANIFEST.write_text(json.dumps(manifest_for(paths), indent=2) + "\n")
    for name, path in paths.items():
        print(f"wrote {path.relative_to(ROOT)} ({path.stat().st_size} bytes, {name})")
    print(f"wrote {MANIFEST.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
