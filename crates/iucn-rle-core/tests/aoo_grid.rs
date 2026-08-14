//! The AOO grid: 10 × 10 km cells snapped to the global origin.
//!
//! The grid is global and fixed, not derived from the data. Cells are indexed from
//! (0, 0) at the projection origin so that the same ground position lands in the
//! same cell in every assessment ever made — which is what makes AOO counts
//! comparable between ecosystems and between assessors.

use iucn_rle_core::grid::{CellId, AOO_CELL_SIZE_M};

#[test]
fn the_cell_size_is_ten_kilometres() {
    // Fixed by the Guidelines (§6.3.2, p. 67), not a tunable parameter.
    assert!((AOO_CELL_SIZE_M - 10_000.0).abs() < f64::EPSILON);
}

#[test]
fn the_origin_is_cell_zero_zero() {
    assert_eq!(CellId::containing(0.0, 0.0), CellId::new(0, 0));
}

#[test]
fn positive_coordinates_index_upward() {
    assert_eq!(CellId::containing(5_000.0, 5_000.0), CellId::new(0, 0));
    assert_eq!(CellId::containing(10_000.0, 0.0), CellId::new(1, 0));
    assert_eq!(CellId::containing(25_000.0, 45_000.0), CellId::new(2, 4));
}

#[test]
fn negative_coordinates_floor_rather_than_truncate() {
    // The bug this test exists to prevent. Casting -5_000.0 / 10_000.0 to an
    // integer truncates toward zero and gives cell 0 — the same cell as +5,000 —
    // silently merging the western and eastern hemispheres, and the southern and
    // northern ones. Half the planet would be misplaced.
    assert_eq!(CellId::containing(-5_000.0, -5_000.0), CellId::new(-1, -1));
    assert_eq!(CellId::containing(-1.0, -1.0), CellId::new(-1, -1));
    assert_eq!(
        CellId::containing(-10_000.0, -10_000.0),
        CellId::new(-1, -1)
    );
    assert_eq!(
        CellId::containing(-10_001.0, -10_001.0),
        CellId::new(-2, -2)
    );
}

#[test]
fn a_cell_owns_its_lower_left_corner_and_not_its_upper_right() {
    // Half-open intervals, so a point on a shared edge belongs to exactly one cell
    // and no ground position is counted twice.
    assert_eq!(CellId::containing(10_000.0, 10_000.0), CellId::new(1, 1));
    assert_eq!(CellId::containing(9_999.999, 9_999.999), CellId::new(0, 0));
}

#[test]
fn bounds_round_trip_through_containing() {
    // Every cell's own lower-left corner must map back to that cell. Cheap, and it
    // catches sign and off-by-one errors across all four quadrants at once.
    for col in [-3, -1, 0, 1, 7] {
        for row in [-5, -1, 0, 2, 11] {
            let cell = CellId::new(col, row);
            let [min_x, min_y, ..] = cell.bounds();
            assert_eq!(CellId::containing(min_x, min_y), cell);
        }
    }
}

#[test]
fn bounds_are_ten_kilometres_square() {
    let [min_x, min_y, max_x, max_y] = CellId::new(-2, 3).bounds();
    assert!((max_x - min_x - AOO_CELL_SIZE_M).abs() < 1e-9);
    assert!((max_y - min_y - AOO_CELL_SIZE_M).abs() < 1e-9);
    assert!((min_x - -20_000.0).abs() < 1e-9);
    assert!((min_y - 30_000.0).abs() < 1e-9);
}

#[test]
fn a_bounding_box_covers_the_cells_it_spans() {
    // Used to decide which cells a feature can possibly touch, without building a
    // grid. Must include partially covered cells at both ends.
    let cells: Vec<CellId> = CellId::covering([-5_000.0, 0.0, 15_000.0, 10_000.0]).collect();

    assert_eq!(cells.len(), 6, "3 columns x 2 rows, got {cells:?}");
    assert!(cells.contains(&CellId::new(-1, 0)));
    assert!(cells.contains(&CellId::new(1, 1)));
}

#[test]
fn a_bounding_box_inside_one_cell_yields_one_cell() {
    let cells: Vec<CellId> = CellId::covering([1_000.0, 1_000.0, 2_000.0, 2_000.0]).collect();
    assert_eq!(cells, vec![CellId::new(0, 0)]);
}

#[test]
fn an_inverted_bounding_box_covers_nothing() {
    // Defensive: a caller computing bounds from an empty geometry can produce this.
    let cells: Vec<CellId> = CellId::covering([10.0, 10.0, 0.0, 0.0]).collect();
    assert!(cells.is_empty());
}

#[test]
fn cells_are_globally_consistent_regardless_of_the_data() {
    // The defining property. Two assessments of different ecosystems that happen to
    // share ground must agree on which cell that ground is in, because the grid is
    // anchored to the projection origin rather than to either dataset's extent.
    let somewhere = (1_234_567.0, -7_654_321.0);
    assert_eq!(
        CellId::containing(somewhere.0, somewhere.1),
        CellId::new(123, -766)
    );
}
