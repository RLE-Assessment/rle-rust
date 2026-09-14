//! Reading a window from a Cloud-Optimized `GeoTIFF` over byte ranges.
//!
//! Fixtures are written by GDAL's own COG driver, so these check agreement with the
//! reference implementation rather than with this crate's assumptions. Regenerate with
//! `python3 tools/generate_cog_fixture.py`.
//!
//! The raster is 512x512 with 256x256 tiles, in ESRI:54034 with 10 km pixels, so one
//! pixel is exactly one AOO cell and a pixel's column and row *are* its grid cell's.
//! Values are `(x / 64) + 8 * (y / 64)`, which varies both within and across tiles — a
//! reader that muddles tile order, or reads an overview instead of full resolution,
//! produces visibly wrong numbers rather than plausible ones.

// Byte offsets are u64 because a real COG can exceed 4 GB; these fixtures are kilobytes
// and the tests run on 64-bit hosts, so narrowing them to index a Vec is exact here.
#![allow(clippy::cast_possible_truncation)]
// Pixel values are small integers held as f64, so they are exactly representable and
// an approximate comparison would weaken the assertion rather than stabilise it.
#![allow(clippy::float_cmp)]

use core::ops::Range;
use std::fs;
use std::path::PathBuf;

use bytes::Bytes;
use iucn_rle_format::cog::{
    header_range, parse_header, Cog, CogError, Header, PixelWindow, DEFAULT_HEADER_PREFETCH,
};
use iucn_rle_format::geoparquet::SparseBytes;

fn read(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/data")
        .join(name);
    fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}. Regenerate with \
             `python3 tools/generate_cog_fixture.py`",
            path.display()
        )
    })
}

/// Open a COG the way a remote reader would: fetch the head, parse, retry if short.
fn open(name: &str) -> (Cog, Vec<u8>) {
    let bytes = read(name);
    let size = bytes.len() as u64;
    let mut range = header_range(size, DEFAULT_HEADER_PREFETCH);
    for _ in 0..8 {
        let head = &bytes[range.start as usize..range.end as usize];
        match parse_header(head, size).unwrap() {
            Header::Complete(cog) => return (*cog, bytes),
            Header::NeedMore(wider) => range = wider,
        }
    }
    panic!("header parsing did not converge for {name}");
}

/// Serve byte ranges out of the whole file, as a real fetch would.
fn fetch(bytes: &[u8], ranges: &[core::ops::Range<u64>]) -> SparseBytes {
    SparseBytes::new(
        bytes.len() as u64,
        ranges
            .iter()
            .map(|range| {
                (
                    range.start,
                    Bytes::copy_from_slice(&bytes[range.start as usize..range.end as usize]),
                )
            })
            .collect(),
    )
}

/// The value the fixture holds at a pixel.
fn expected(x: u32, y: u32) -> f64 {
    f64::from((x / 64) + 8 * (y / 64))
}

#[test]
fn the_full_resolution_image_is_described() {
    let (cog, _) = open("ecosystems_cog.tif");

    assert_eq!(cog.width(), 512);
    assert_eq!(cog.height(), 512);
    assert_eq!(cog.tile_size(), (256, 256));
}

#[test]
fn the_first_image_is_used_not_an_overview() {
    // A COG stores reduced-resolution overviews as further IFDs after the first. Both
    // decode without error, so reading the wrong one gives a plausible raster at the
    // wrong scale — every AOO cell count would be wrong by a power of two.
    let (cog, _) = open("ecosystems_cog.tif");

    assert_eq!(
        (cog.width(), cog.height()),
        (512, 512),
        "the overview is 256x256; reading it would silently halve the resolution"
    );
}

#[test]
fn the_geotransform_places_the_raster_in_the_world() {
    let (cog, _) = open("ecosystems_cog.tif");

    assert_eq!(cog.origin(), (0.0, 0.0));
    assert_eq!(cog.pixel_size(), (10_000.0, -10_000.0));
}

#[test]
fn the_projection_is_recognised_as_the_aoo_grid_crs() {
    // The load-bearing check, and the reason this cannot be done by looking up a code.
    // GDAL writes ESRI:54034 with ProjectedCSTypeGeoKey = 32767 (user-defined) — there
    // is no EPSG or ESRI code anywhere in the file. The CRS is identifiable only from
    // the transformation type and its parameters, which is exactly why
    // `@developmentseed/geotiff` rejects these files with "Unsupported coordinate
    // transformation type: 28".
    let (cog, _) = open("ecosystems_cog.tif");

    assert!(
        cog.is_aoo_crs(),
        "should be recognised as ESRI:54034, but was {:?}",
        cog.crs_description()
    );
}

#[test]
fn a_geographic_raster_is_refused_rather_than_silently_accepted() {
    // Pixels in EPSG:4326 have curved edges in the equal-area plane, so every pixel
    // would need approximating. Accepting it would give numbers that look reasonable
    // and are wrong.
    let (cog, _) = open("ecosystems_cog_wgs84.tif");

    assert!(
        !cog.is_aoo_crs(),
        "a geographic raster is not the AOO grid CRS"
    );
}

#[test]
fn a_window_selects_only_the_tiles_it_covers() {
    // The whole point of tiling. A window inside one tile must not fetch the other three.
    let (cog, _) = open("ecosystems_cog.tif");
    let tiles = cog.tiles_for(PixelWindow::new(10, 10, 50, 50));

    assert_eq!(tiles, vec![0], "entirely inside the top-left tile");
}

#[test]
fn a_window_spanning_a_tile_boundary_selects_both_tiles() {
    let (cog, _) = open("ecosystems_cog.tif");
    let tiles = cog.tiles_for(PixelWindow::new(250, 10, 20, 20));

    assert_eq!(tiles, vec![0, 1], "crosses x = 256");
}

#[test]
fn a_window_covering_everything_selects_every_tile() {
    let (cog, _) = open("ecosystems_cog.tif");
    let tiles = cog.tiles_for(PixelWindow::new(0, 0, 512, 512));

    assert_eq!(tiles, vec![0, 1, 2, 3]);
}

#[test]
fn a_window_outside_the_raster_selects_nothing() {
    let (cog, _) = open("ecosystems_cog.tif");
    let tiles = cog.tiles_for(PixelWindow::new(600, 600, 10, 10));

    assert!(tiles.is_empty());
}

#[test]
fn reading_one_tile_fetches_far_less_than_the_whole_raster() {
    // Asserted on the uncompressed fixture, where bytes map directly to pixels and the
    // saving is real rather than an artefact of the header dominating a tiny file.
    let (cog, bytes) = open("ecosystems_cog_uncompressed.tif");
    let ranges = cog.ranges_for(&cog.tiles_for(PixelWindow::new(0, 0, 100, 100)));
    let fetched: u64 = ranges.iter().map(|range| range.end - range.start).sum();

    assert!(
        fetched * 3 < bytes.len() as u64,
        "fetched {fetched} of {} bytes for one tile of four",
        bytes.len()
    );
}

#[test]
fn a_window_decodes_to_the_values_that_were_written() {
    let (cog, bytes) = open("ecosystems_cog.tif");
    let window = PixelWindow::new(0, 0, 4, 4);
    let ranges = cog.ranges_for(&cog.tiles_for(window));

    let raster = cog.decode(window, &fetch(&bytes, &ranges)).unwrap();

    assert_eq!(raster.width, 4);
    assert_eq!(raster.height, 4);
    assert_eq!(raster.values[0], expected(0, 0));
}

#[test]
fn a_window_crossing_tiles_is_assembled_in_the_right_order() {
    // The place a tiled reader most often goes wrong: pixels from the second tile
    // written at the wrong offset, giving a mirrored or shifted raster that still has
    // the right histogram, so totals look correct while locations are not.
    let (cog, bytes) = open("ecosystems_cog.tif");
    let window = PixelWindow::new(254, 254, 4, 4);
    let ranges = cog.ranges_for(&cog.tiles_for(window));

    let raster = cog.decode(window, &fetch(&bytes, &ranges)).unwrap();

    for row in 0..4u32 {
        for col in 0..4u32 {
            let (x, y) = (254 + col, 254 + row);
            assert_eq!(
                raster.values[(row * 4 + col) as usize],
                expected(x, y),
                "pixel ({x}, {y})"
            );
        }
    }
}

#[test]
fn every_probe_point_decodes_to_its_recorded_value() {
    // The probes are recorded in fixtures/cases/cog.json by the generator, so this
    // checks against measured truth rather than a formula reimplemented on both sides.
    let (cog, bytes) = open("ecosystems_cog.tif");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cases/cog.json"),
        )
        .unwrap(),
    )
    .unwrap();

    for probe in manifest["probes"].as_array().unwrap() {
        let (x, y) = (
            probe["x"].as_u64().unwrap() as u32,
            probe["y"].as_u64().unwrap() as u32,
        );
        let value = probe["value"].as_f64().unwrap();

        let window = PixelWindow::new(x, y, 1, 1);
        let ranges = cog.ranges_for(&cog.tiles_for(window));
        let raster = cog.decode(window, &fetch(&bytes, &ranges)).unwrap();

        assert_eq!(raster.values[0], value, "at pixel ({x}, {y})");
    }
}

#[test]
fn an_uncompressed_raster_decodes_identically() {
    // Compression is the likeliest place a decoder differs, so the two fixtures must
    // agree pixel for pixel.
    let window = PixelWindow::new(100, 100, 8, 8);

    let (deflate, deflate_bytes) = open("ecosystems_cog.tif");
    let from_deflate = deflate
        .decode(
            window,
            &fetch(
                &deflate_bytes,
                &deflate.ranges_for(&deflate.tiles_for(window)),
            ),
        )
        .unwrap();

    let (plain, plain_bytes) = open("ecosystems_cog_uncompressed.tif");
    let from_plain = plain
        .decode(
            window,
            &fetch(&plain_bytes, &plain.ranges_for(&plain.tiles_for(window))),
        )
        .unwrap();

    assert_eq!(from_deflate.values, from_plain.values);
}

#[test]
fn a_window_reports_the_projected_position_of_its_own_corner() {
    // The decoded window has to know where it is, or the accumulator cannot place its
    // pixels in grid cells. With 10 km pixels from the origin, pixel (30, 40) starts at
    // (300 000, -400 000).
    let (cog, bytes) = open("ecosystems_cog.tif");
    let window = PixelWindow::new(30, 40, 2, 2);
    let ranges = cog.ranges_for(&cog.tiles_for(window));

    let raster = cog.decode(window, &fetch(&bytes, &ranges)).unwrap();

    assert_eq!(raster.origin_x, 300_000.0);
    assert_eq!(raster.origin_y, -400_000.0);
    assert_eq!(raster.pixel_width, 10_000.0);
    assert_eq!(raster.pixel_height, -10_000.0);
}

#[test]
fn a_window_partly_outside_the_raster_is_clipped_to_it() {
    // Asking near the edge is ordinary. Returning nodata past the edge would add
    // phantom pixels; erroring would make edge queries a special case for callers.
    let (cog, bytes) = open("ecosystems_cog.tif");
    let window = PixelWindow::new(508, 508, 16, 16);
    let ranges = cog.ranges_for(&cog.tiles_for(window));

    let raster = cog.decode(window, &fetch(&bytes, &ranges)).unwrap();

    assert_eq!(
        (raster.width, raster.height),
        (4, 4),
        "clipped to the raster"
    );
    assert_eq!(raster.values[15], expected(511, 511));
}

#[test]
fn decoding_without_the_needed_bytes_is_an_error_not_a_blank_raster() {
    // Silently returning zeros would read as "no ecosystem here", which is exactly the
    // wrong answer to give an assessment.
    //
    // Uses the uncompressed fixture deliberately: the DEFLATE one is smaller than the
    // header prefetch, so "fetch only the header" would fetch the entire file and the
    // test would prove nothing.
    let (cog, bytes) = open("ecosystems_cog_uncompressed.tif");
    let window = PixelWindow::new(400, 400, 4, 4);

    // Fetch the header but none of the tile the window needs.
    let header_only = [Range {
        start: 0,
        end: cog.header_length(),
    }];
    let head = fetch(&bytes, &header_only);
    let err = cog.decode(window, &head).unwrap_err();

    assert!(matches!(err, CogError::Read { .. }), "{err:?}");
}

#[test]
fn a_short_header_asks_for_a_range_that_works() {
    // Same contract as the parquet footer: NeedMore must name a range that actually
    // succeeds, or a reader loops for ever.
    let bytes = read("ecosystems_cog.tif");
    let size = bytes.len() as u64;

    let Header::NeedMore(wider) = parse_header(&bytes[..64], size).unwrap() else {
        panic!("64 bytes should not be enough to parse the IFD");
    };
    assert!(wider.end <= size);
    assert!(
        wider.end - wider.start > 64,
        "it must ask for more, not less"
    );

    let retry = &bytes[wider.start as usize..wider.end as usize];
    assert!(
        matches!(parse_header(retry, size), Ok(Header::Complete(_))),
        "the range NeedMore asked for must be sufficient"
    );
}

#[test]
fn bytes_that_are_not_a_tiff_are_rejected_by_name() {
    let err = parse_header(b"<!DOCTYPE html><html>not a tiff at all</html>", 44).unwrap_err();
    assert!(matches!(err, CogError::NotTiff { .. }), "{err:?}");
}

#[test]
fn a_mask_selects_only_the_requested_ecosystem() {
    // How a class raster becomes something the AOO accumulator can take: coverage is
    // 1.0 where the pixel holds that class and 0.0 elsewhere.
    let (cog, bytes) = open("ecosystems_cog.tif");
    let window = PixelWindow::new(60, 0, 8, 1);
    let ranges = cog.ranges_for(&cog.tiles_for(window));
    let raster = cog.decode(window, &fetch(&bytes, &ranges)).unwrap();

    // x = 60..63 hold class 0; x = 64..67 hold class 1.
    let mask = raster.mask(0.0);

    assert_eq!(mask[..4], [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(mask[4..], [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn a_decoded_window_feeds_the_aoo_accumulator_directly() {
    // The join between this crate and the core: one 10 km pixel is one AOO cell, so a
    // 3x2 window of a single class must occupy exactly six cells.
    use iucn_rle_core::aoo::AooAccumulator;

    let (cog, bytes) = open("ecosystems_cog.tif");
    let window = PixelWindow::new(0, 0, 3, 2);
    let ranges = cog.ranges_for(&cog.tiles_for(window));
    let raster = cog.decode(window, &fetch(&bytes, &ranges)).unwrap();

    let mask = raster.mask(0.0);
    let mut accumulator = AooAccumulator::new();
    accumulator.add_raster("class-0", &raster.as_window(&mask));

    assert_eq!(
        accumulator.finish().aoo("class-0").occupied_cell_count,
        6,
        "one 10 km pixel is exactly one AOO cell"
    );
}
