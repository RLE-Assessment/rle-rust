//! Reading a remote raster into an assessment, one tile at a time.
//!
//! The vector path had an engine entry point from the start; the raster one did not,
//! so a COG could be decoded but never actually assessed. Same loop shape, same reason:
//! fetch one tile, fold it in, drop it.
//!
//! The fixture is 512x512 with 10 km pixels on the AOO grid's own origin, so one pixel
//! is exactly one grid cell and the expected counts are arithmetic rather than a
//! recorded blob. Class `c` occupies a 64x64 block, so 4096 cells.

#![allow(clippy::cast_possible_truncation)]

use std::fs;
use std::path::PathBuf;

use futures::executor::block_on;
use iucn_rle_core::aoo::AooAccumulator;
use iucn_rle_engine::{accumulate_cog, EngineError};
use iucn_rle_io::InMemorySource;

fn source(name: &str) -> InMemorySource {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/data")
        .join(name);
    InMemorySource::new(
        fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display())),
    )
}

#[test]
fn a_class_occupies_the_cells_its_pixels_cover() {
    // Class 0 fills x 0..63, y 0..63 — a 64x64 block of 10 km pixels, and therefore
    // exactly 4096 grid cells.
    let mut accumulator = AooAccumulator::new();

    let report = block_on(accumulate_cog(
        &source("ecosystems_cog.tif"),
        "class-0",
        0.0,
        &mut accumulator,
    ))
    .unwrap();

    // Every tile is read, not just the one holding the class. A COG carries no
    // per-tile statistics — nothing equivalent to a row group's min/max — so there is
    // nothing to prune on, and a reader cannot know a tile is irrelevant without
    // decoding it. The bounded thing here is memory, not bytes transferred.
    assert_eq!(
        report.tiles_read, 4,
        "all four tiles, for want of statistics"
    );
    assert_eq!(
        accumulator.finish().aoo("class-0").occupied_cell_count,
        4096
    );
}

#[test]
fn a_class_spanning_tiles_is_accumulated_across_them() {
    // Class 36 sits at x 256..319, y 256..319 — inside the bottom-right tile. Class 4
    // spans the top two tiles' boundary region, so it exercises the seam.
    let mut accumulator = AooAccumulator::new();

    block_on(accumulate_cog(
        &source("ecosystems_cog.tif"),
        "class-4",
        4.0,
        &mut accumulator,
    ))
    .unwrap();

    assert_eq!(
        accumulator.finish().aoo("class-4").occupied_cell_count,
        4096,
        "every class covers the same 64x64 block, wherever it sits"
    );
}

#[test]
fn a_class_that_is_absent_occupies_nothing() {
    // A legitimate question with a legitimately empty answer, not an error.
    let mut accumulator = AooAccumulator::new();

    let report = block_on(accumulate_cog(
        &source("ecosystems_cog.tif"),
        "absent",
        999.0,
        &mut accumulator,
    ))
    .unwrap();

    assert!(report.tiles_read > 0, "it still had to look");
    assert_eq!(accumulator.finish().aoo("absent").occupied_cell_count, 0);
}

#[test]
fn tiles_are_fetched_one_at_a_time() {
    // The same memory claim as the vector path. A reader that gathered every tile
    // before decoding would return identical counts.
    let mut accumulator = AooAccumulator::new();

    let report = block_on(accumulate_cog(
        &source("ecosystems_cog_uncompressed.tif"),
        "class-0",
        0.0,
        &mut accumulator,
    ))
    .unwrap();

    assert!(
        report.peak_tile_bytes * 3 < report.bytes_fetched,
        "peak tile was {} of {} bytes fetched, which is not streaming",
        report.peak_tile_bytes,
        report.bytes_fetched
    );
}

#[test]
fn a_raster_in_the_wrong_projection_is_refused() {
    // Pixels are only rectangles in the grid's own plane. Accepting a geographic raster
    // would give cell counts that look plausible and are wrong.
    let mut accumulator = AooAccumulator::new();

    let error = block_on(accumulate_cog(
        &source("ecosystems_cog_wgs84.tif"),
        "class-0",
        0.0,
        &mut accumulator,
    ))
    .unwrap_err();

    assert!(matches!(error, EngineError::NotGridCrs { .. }), "{error:?}");
}

#[test]
fn bytes_that_are_not_a_raster_are_reported_clearly() {
    let mut accumulator = AooAccumulator::new();

    let error = block_on(accumulate_cog(
        &InMemorySource::new(b"<!DOCTYPE html><html>404</html>".to_vec()),
        "class-0",
        0.0,
        &mut accumulator,
    ))
    .unwrap_err();

    assert!(matches!(error, EngineError::Cog(_)), "{error:?}");
}
