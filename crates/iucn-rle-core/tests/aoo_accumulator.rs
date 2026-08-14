//! Accumulating per-cell ecosystem extent, one feature at a time.
//!
//! This is the piece that replaces `rle-python`'s memory problem. That implementation
//! holds the national vector twice — once geographic, once equal-area — spatially
//! joins it against a materialised grid, then pivots to one column per ecosystem.
//! Here, features are consumed one at a time and only *occupied* cells are retained,
//! so memory scales with the ecosystem's footprint rather than with the size of the
//! source map.

use iucn_rle_core::aoo::AooAccumulator;
use iucn_rle_core::grid::CellId;

/// A counter-clockwise square, in metres.
fn square(min_x: f64, min_y: f64, size: f64) -> Vec<[f64; 2]> {
    vec![
        [min_x, min_y],
        [min_x + size, min_y],
        [min_x + size, min_y + size],
        [min_x, min_y + size],
    ]
}

fn approx(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn an_empty_accumulator_yields_nothing() {
    let grid = AooAccumulator::new().finish();
    assert!(grid.cells().is_empty());
    assert_eq!(grid.aoo("anything").aoo_cells, 0);
}

#[test]
fn a_feature_inside_one_cell_occupies_that_cell() {
    let mut acc = AooAccumulator::new();
    acc.add_polygon("forest", &[square(2_000.0, 2_000.0, 1_000.0)]);
    let grid = acc.finish();

    assert_eq!(grid.cells().len(), 1);
    let cell = &grid.cells()[0];
    assert_eq!(cell.cell, CellId::new(0, 0));
    assert_eq!(cell.ecosystem, "forest");
    approx(cell.extent_m2, 1_000_000.0);
    approx(cell.fraction, 0.01); // 1 km² of a 100 km² cell
}

#[test]
fn a_feature_spanning_cells_is_split_between_them() {
    // Straddles the boundary between cells (0,0) and (1,0), 4 km each side.
    let mut acc = AooAccumulator::new();
    acc.add_polygon("forest", &[square(6_000.0, 1_000.0, 8_000.0)]);
    let grid = acc.finish();

    assert_eq!(grid.cells().len(), 2);

    let total: f64 = grid.cells().iter().map(|c| c.extent_m2).sum();
    approx(total, 8_000.0 * 8_000.0);
}

#[test]
fn separate_features_of_one_ecosystem_accumulate() {
    let mut acc = AooAccumulator::new();
    acc.add_polygon("forest", &[square(1_000.0, 1_000.0, 1_000.0)]);
    acc.add_polygon("forest", &[square(5_000.0, 5_000.0, 2_000.0)]);
    let grid = acc.finish();

    // Both fall in the same cell, so they sum into one entry.
    assert_eq!(grid.cells().len(), 1);
    approx(grid.cells()[0].extent_m2, 1_000_000.0 + 4_000_000.0);
}

#[test]
fn different_ecosystems_stay_separate() {
    let mut acc = AooAccumulator::new();
    acc.add_polygon("forest", &[square(1_000.0, 1_000.0, 1_000.0)]);
    acc.add_polygon("grassland", &[square(2_000.0, 2_000.0, 1_000.0)]);
    let grid = acc.finish();

    // Same cell, two ecosystems: two rows, not one merged row. This is the
    // long/tidy shape — rle-python pivots to one column per ecosystem, which for
    // Colombia means 161 mostly-zero columns.
    assert_eq!(grid.cells().len(), 2);
    assert_eq!(grid.ecosystems().len(), 2);
    assert_eq!(grid.aoo("forest").occupied_cell_count, 1);
    assert_eq!(grid.aoo("grassland").occupied_cell_count, 1);
}

#[test]
fn a_polygon_with_a_hole_contributes_only_the_ring_between() {
    let mut acc = AooAccumulator::new();
    let exterior = square(1_000.0, 1_000.0, 6_000.0);
    let mut hole = square(2_000.0, 2_000.0, 2_000.0);
    hole.reverse(); // clockwise, so it subtracts

    acc.add_polygon("forest", &[exterior, hole]);
    let grid = acc.finish();

    approx(grid.cells()[0].extent_m2, 36_000_000.0 - 4_000_000.0);
}

#[test]
fn a_cell_entirely_within_a_hole_is_not_occupied() {
    // The exterior covers the cell and the hole removes it again, netting zero. A
    // cell nets to nothing must not be reported as occupied.
    let mut acc = AooAccumulator::new();
    let exterior = square(-15_000.0, -15_000.0, 45_000.0);
    let mut hole = square(-5_000.0, -5_000.0, 25_000.0);
    hole.reverse();

    acc.add_polygon("forest", &[exterior, hole]);
    let grid = acc.finish();

    let occupied: Vec<CellId> = grid.cells().iter().map(|c| c.cell).collect();
    assert!(
        !occupied.contains(&CellId::new(0, 0)),
        "cell (0,0) is inside the hole and should not be occupied"
    );
    assert!(!occupied.contains(&CellId::new(1, 1)));
}

#[test]
fn overlapping_features_are_capped_at_the_cell_and_reported() {
    // Overlapping polygons of the same ecosystem are a data-quality problem: the
    // true extent is their union, but computing that needs a boolean-overlay engine
    // this library deliberately does not carry. Summing is exact for a clean,
    // non-overlapping coverage — the normal case — so we sum, cap at the cell's own
    // area since a cell cannot be more than fully occupied, and *report* that it
    // happened rather than silently hiding a broken input.
    let mut acc = AooAccumulator::new();
    acc.add_polygon("forest", &[square(0.0, 0.0, 10_000.0)]);
    acc.add_polygon("forest", &[square(0.0, 0.0, 10_000.0)]);
    let grid = acc.finish();

    approx(grid.cells()[0].extent_m2, 100_000_000.0);
    approx(grid.cells()[0].fraction, 1.0);
    assert_eq!(
        grid.overfull_cells(),
        1,
        "the overlap must be surfaced, not swallowed"
    );
}

#[test]
fn a_clean_coverage_reports_no_overfull_cells() {
    let mut acc = AooAccumulator::new();
    acc.add_polygon("forest", &[square(0.0, 0.0, 5_000.0)]);
    acc.add_polygon("forest", &[square(5_000.0, 0.0, 5_000.0)]);
    let grid = acc.finish();

    assert_eq!(grid.overfull_cells(), 0);
    approx(grid.cells()[0].fraction, 0.5);
}

#[test]
fn both_winding_conventions_give_the_same_answer() {
    // Not a theoretical concern. GeoJSON specifies counter-clockwise exterior rings
    // (RFC 7946 §3.1.6); shapefiles use clockwise ones. If the accumulator assumed
    // one convention, every polygon from the other would produce negative area, be
    // filtered out as unoccupied, and yield a silently empty result for an entire
    // dataset — an AOO of zero rather than an error.
    let accumulate = |ring: Vec<[f64; 2]>| {
        let mut acc = AooAccumulator::new();
        acc.add_polygon("forest", &[ring]);
        acc.finish()
    };

    let counter_clockwise = square(1_000.0, 1_000.0, 4_000.0);
    let mut clockwise = counter_clockwise.clone();
    clockwise.reverse();

    let from_geojson = accumulate(counter_clockwise);
    let from_shapefile = accumulate(clockwise);

    assert_eq!(
        from_shapefile.cells().len(),
        1,
        "clockwise input must not vanish"
    );
    approx(
        from_shapefile.cells()[0].extent_m2,
        from_geojson.cells()[0].extent_m2,
    );
    approx(from_shapefile.cells()[0].extent_m2, 16_000_000.0);
}

#[test]
fn holes_still_subtract_when_the_exterior_is_clockwise() {
    // Winding normalisation must not break holes: a hole is defined as opposite to
    // its exterior, so flipping the exterior flips the hole too.
    let mut exterior = square(1_000.0, 1_000.0, 6_000.0);
    exterior.reverse(); // clockwise exterior, shapefile style
    let hole = square(2_000.0, 2_000.0, 2_000.0); // counter-clockwise, so opposite

    let mut acc = AooAccumulator::new();
    acc.add_polygon("forest", &[exterior, hole]);
    let grid = acc.finish();

    approx(grid.cells()[0].extent_m2, 36_000_000.0 - 4_000_000.0);
}

#[test]
fn only_occupied_cells_are_stored() {
    // The memory property. A feature whose bounding box spans a hundred cells but
    // which only touches a handful must retain only the handful.
    let mut acc = AooAccumulator::new();
    // A thin diagonal sliver crossing a 10x10 cell bounding box.
    acc.add_polygon(
        "forest",
        &[vec![
            [1_000.0, 1_000.0],
            [99_000.0, 99_000.0],
            [99_000.0, 98_000.0],
        ]],
    );
    let grid = acc.finish();

    assert!(
        grid.cells().len() < 40,
        "a diagonal sliver should not retain all 100 cells of its bbox, got {}",
        grid.cells().len()
    );
    assert!(!grid.cells().is_empty());
}

#[test]
fn the_aoo_applies_the_one_percent_exclusion() {
    // End to end: geometry in, Criterion B2 number out.
    let mut acc = AooAccumulator::new();
    // Three substantial cells...
    for col in 0..3 {
        acc.add_polygon(
            "forest",
            &[square(
                f64::from(col) * 10_000.0 + 1_000.0,
                1_000.0,
                8_000.0,
            )],
        );
    }
    // ...and one negligible sliver far away.
    acc.add_polygon("forest", &[square(500_000.0, 500_000.0, 10.0)]);

    let grid = acc.finish();
    let aoo = grid.aoo("forest");

    assert_eq!(aoo.occupied_cell_count, 4, "cells actually touched");
    assert_eq!(aoo.aoo_cells, 3, "the sliver is excluded from Criterion B2");
}

#[test]
fn an_unknown_ecosystem_has_no_area_of_occupancy() {
    let mut acc = AooAccumulator::new();
    acc.add_polygon("forest", &[square(1_000.0, 1_000.0, 1_000.0)]);
    let grid = acc.finish();

    assert_eq!(grid.aoo("nonexistent").aoo_cells, 0);
}

#[test]
fn cells_come_back_in_a_deterministic_order() {
    // The accumulator is hash-backed, so output order would otherwise vary run to
    // run. Assessments get committed to repositories and diffed; unstable ordering
    // would produce noise in every rebuild.
    let build = || {
        let mut acc = AooAccumulator::new();
        for col in 0..5 {
            for row in 0..5 {
                acc.add_polygon(
                    "forest",
                    &[square(
                        f64::from(col) * 10_000.0 + 1_000.0,
                        f64::from(row) * 10_000.0 + 1_000.0,
                        2_000.0,
                    )],
                );
            }
        }
        acc.finish()
    };

    let first: Vec<CellId> = build().cells().iter().map(|c| c.cell).collect();
    let second: Vec<CellId> = build().cells().iter().map(|c| c.cell).collect();
    assert_eq!(first, second);
}
