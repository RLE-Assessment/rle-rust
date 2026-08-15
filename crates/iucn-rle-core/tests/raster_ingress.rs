//! Accumulating AOO from a raster rather than from polygons.
//!
//! Many distribution maps are rasters, and `rle-python` rasterises vectors to a COG
//! as a matter of course. This is the bring-your-own-data path: a caller hands over
//! an array they already have — a numpy array, `terra` values, a typed array — with
//! no file and no URL involved.
//!
//! The raster must already be in the AOO grid's CRS. That is not a shortcut: pixels
//! are then axis-aligned rectangles in the same plane as the cells, so the overlap is
//! an exact interval intersection rather than an approximation. `rle-python` takes the
//! same approach, reprojecting to ESRI:54034 before any zonal statistics.

use iucn_rle_core::aoo::{AooAccumulator, RasterWindow};

/// A north-up window whose pixels are `pixel_m` metres square, with its top-left
/// corner at `(origin_x, origin_y)` in projected metres.
fn window(
    data: &[f64],
    width: usize,
    origin_x: f64,
    origin_y: f64,
    pixel_m: f64,
) -> RasterWindow<'_> {
    RasterWindow {
        data,
        width,
        height: data.len().checked_div(width).unwrap_or(0),
        origin_x,
        origin_y,
        pixel_width: pixel_m,
        // Negative: raster rows run north to south, the near-universal convention.
        pixel_height: -pixel_m,
    }
}

fn approx(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn a_fully_covered_pixel_contributes_its_whole_area() {
    // One 1 km pixel, fully occupied, inside cell (0, 0).
    let data = [1.0];
    let mut acc = AooAccumulator::new();
    acc.add_raster("forest", &window(&data, 1, 1_000.0, 2_000.0, 1_000.0));
    let grid = acc.finish();

    assert_eq!(grid.cells().len(), 1);
    approx(grid.cells()[0].extent_m2, 1_000_000.0);
}

#[test]
fn partial_coverage_scales_the_contribution() {
    // Fractional coverage is how a rasterised vector records a pixel that is only
    // partly the ecosystem. A quarter-covered pixel contributes a quarter.
    let data = [0.25];
    let mut acc = AooAccumulator::new();
    acc.add_raster("forest", &window(&data, 1, 1_000.0, 2_000.0, 1_000.0));
    let grid = acc.finish();

    approx(grid.cells()[0].extent_m2, 250_000.0);
}

#[test]
fn empty_pixels_do_not_make_a_cell_occupied() {
    let data = [0.0, 0.0, 0.0, 0.0];
    let mut acc = AooAccumulator::new();
    acc.add_raster("forest", &window(&data, 2, 1_000.0, 2_000.0, 1_000.0));
    let grid = acc.finish();

    assert!(grid.cells().is_empty());
}

#[test]
fn nodata_pixels_are_skipped() {
    // NaN is the conventional nodata marker for float rasters, and must not
    // contaminate the accumulated extent.
    let data = [1.0, f64::NAN, 1.0];
    let mut acc = AooAccumulator::new();
    acc.add_raster("forest", &window(&data, 3, 1_000.0, 2_000.0, 1_000.0));
    let grid = acc.finish();

    approx(grid.cells()[0].extent_m2, 2_000_000.0);
}

#[test]
fn pixels_are_assigned_to_the_cell_they_fall_in() {
    // Two pixels either side of the boundary between cells (0,0) and (1,0).
    let data = [1.0, 1.0];
    let mut acc = AooAccumulator::new();
    acc.add_raster("forest", &window(&data, 2, 9_000.0, 5_000.0, 1_000.0));
    let grid = acc.finish();

    assert_eq!(grid.cells().len(), 2, "one pixel in each cell");
    for cell in grid.cells() {
        approx(cell.extent_m2, 1_000_000.0);
    }
}

#[test]
fn a_pixel_straddling_a_cell_boundary_is_split_exactly() {
    // The reason the raster must already be in the grid's CRS: pixels and cells are
    // then both axis-aligned rectangles, so the overlap is an exact interval
    // intersection with no approximation and no clipping algorithm.
    //
    // A 4 km pixel starting 2 km before the boundary puts half in each cell.
    let data = [1.0];
    let mut acc = AooAccumulator::new();
    acc.add_raster("forest", &window(&data, 1, 8_000.0, 5_000.0, 4_000.0));
    let grid = acc.finish();

    assert_eq!(grid.cells().len(), 2);
    let total: f64 = grid.cells().iter().map(|c| c.extent_m2).sum();
    approx(total, 16_000_000.0);
    for cell in grid.cells() {
        approx(cell.extent_m2, 8_000_000.0);
    }
}

#[test]
fn area_is_conserved_across_a_large_window() {
    // The invariant the whole computation rests on, as for polygons: every pixel's
    // area must land somewhere, and the total must survive.
    let width = 37;
    let height = 23;
    let data = vec![1.0; width * height];
    let pixel_m = 900.0;

    let mut acc = AooAccumulator::new();
    acc.add_raster("forest", &window(&data, width, -12_345.0, 6_789.0, pixel_m));
    let grid = acc.finish();

    let total: f64 = grid.cells().iter().map(|c| c.extent_m2).sum();
    approx(total, (width * height) as f64 * pixel_m * pixel_m);
}

#[test]
fn south_up_rasters_are_handled() {
    // A positive pixel height means rows run south to north. Rare, but real, and
    // silently mirroring the data would put an ecosystem in the wrong hemisphere.
    let data = [1.0, 0.0];
    let mut north_up = AooAccumulator::new();
    north_up.add_raster(
        "forest",
        &RasterWindow {
            data: &data,
            width: 1,
            height: 2,
            origin_x: 1_000.0,
            origin_y: 25_000.0,
            pixel_width: 1_000.0,
            pixel_height: -1_000.0,
        },
    );

    let occupied: Vec<i32> = north_up
        .finish()
        .cells()
        .iter()
        .map(|c| c.cell.row)
        .collect();
    assert_eq!(occupied, vec![2], "the occupied pixel is the northern one");
}

#[test]
fn negative_coverage_is_ignored() {
    // Defensive: a differenced or badly scaled raster can carry negatives, and
    // subtracting extent would silently understate the AOO.
    let data = [-1.0, 1.0];
    let mut acc = AooAccumulator::new();
    acc.add_raster("forest", &window(&data, 2, 1_000.0, 2_000.0, 1_000.0));
    let grid = acc.finish();

    approx(grid.cells()[0].extent_m2, 1_000_000.0);
}

#[test]
fn a_raster_and_an_equivalent_polygon_agree() {
    // The two ingress paths must not disagree. A 4 km square as one polygon, and the
    // same square as a 4x4 grid of fully-covered 1 km pixels.
    let mut from_polygon = AooAccumulator::new();
    from_polygon.add_polygon(
        "forest",
        &[vec![
            [2_000.0, 2_000.0],
            [6_000.0, 2_000.0],
            [6_000.0, 6_000.0],
            [2_000.0, 6_000.0],
        ]],
    );

    let data = vec![1.0; 16];
    let mut from_raster = AooAccumulator::new();
    from_raster.add_raster("forest", &window(&data, 4, 2_000.0, 6_000.0, 1_000.0));

    let polygon_grid = from_polygon.finish();
    let raster_grid = from_raster.finish();

    assert_eq!(polygon_grid.cells().len(), raster_grid.cells().len());
    approx(
        raster_grid.cells()[0].extent_m2,
        polygon_grid.cells()[0].extent_m2,
    );
}

#[test]
fn an_empty_window_is_harmless() {
    let mut acc = AooAccumulator::new();
    acc.add_raster("forest", &window(&[], 0, 0.0, 0.0, 100.0));
    assert!(acc.finish().cells().is_empty());
}
