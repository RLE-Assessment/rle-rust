//! Clipping a polygon ring against a grid cell, and measuring what is inside.
//!
//! This is the inner loop of the AOO computation: for every feature, how much of it
//! falls in each cell it touches. A grid cell is an axis-aligned rectangle, which is
//! convex, so Sutherland-Hodgman clipping is exact and closed-form here — no noding,
//! no snapping, no robustness predicates, and no failure mode. That is why the
//! library does not need a general boolean-overlay engine.

use iucn_rle_core::geometry::{clipped_area, ring_area};

/// A 10 km cell at the origin, matching the AOO grid.
const CELL: [f64; 4] = [0.0, 0.0, 10_000.0, 10_000.0];
/// Its area in m².
const CELL_AREA: f64 = 100_000_000.0;

fn square(min_x: f64, min_y: f64, size: f64) -> Vec<[f64; 2]> {
    // Counter-clockwise, the positive winding.
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
fn a_ring_entirely_inside_contributes_its_whole_area() {
    let ring = square(2_000.0, 2_000.0, 1_000.0);
    approx(clipped_area(&ring, CELL), 1_000_000.0);
}

#[test]
fn a_ring_entirely_outside_contributes_nothing() {
    let ring = square(50_000.0, 50_000.0, 1_000.0);
    approx(clipped_area(&ring, CELL), 0.0);
}

#[test]
fn a_ring_covering_the_cell_contributes_the_whole_cell() {
    // A national polygon swallowing a cell whole: the cell is fully occupied, and
    // the contribution is capped at the cell's own area, never the polygon's.
    let ring = square(-100_000.0, -100_000.0, 500_000.0);
    approx(clipped_area(&ring, CELL), CELL_AREA);
}

#[test]
fn a_ring_straddling_one_edge_contributes_only_the_part_inside() {
    // Half in, half out: 2 km wide, 1 km of which is inside, by 4 km tall.
    let ring = vec![
        [-1_000.0, 1_000.0],
        [1_000.0, 1_000.0],
        [1_000.0, 5_000.0],
        [-1_000.0, 5_000.0],
    ];
    approx(clipped_area(&ring, CELL), 1_000.0 * 4_000.0);
}

#[test]
fn a_ring_straddling_a_corner_contributes_the_corner_only() {
    let ring = square(-2_000.0, -2_000.0, 4_000.0);
    approx(clipped_area(&ring, CELL), 2_000.0 * 2_000.0);
}

#[test]
fn a_ring_touching_only_an_edge_contributes_nothing() {
    // Shares the boundary but has no interior inside. A zero-area contribution
    // must not make the cell look occupied.
    let ring = square(-1_000.0, 2_000.0, 1_000.0);
    approx(clipped_area(&ring, CELL), 0.0);
}

#[test]
fn a_triangle_is_measured_correctly() {
    let ring = vec![[0.0, 0.0], [4_000.0, 0.0], [0.0, 3_000.0]];
    approx(clipped_area(&ring, CELL), 0.5 * 4_000.0 * 3_000.0);
}

#[test]
fn winding_determines_the_sign() {
    // Signed area is what lets holes work: a caller sums an exterior ring and its
    // holes, and the holes subtract themselves without any special handling.
    let counter_clockwise = square(1_000.0, 1_000.0, 2_000.0);
    let mut clockwise = counter_clockwise.clone();
    clockwise.reverse();

    approx(ring_area(&counter_clockwise), 4_000_000.0);
    approx(ring_area(&clockwise), -4_000_000.0);
    approx(clipped_area(&clockwise, CELL), -4_000_000.0);
}

#[test]
fn a_polygon_with_a_hole_sums_to_the_area_between() {
    // The composition property holes rely on. A 4 km square with a 2 km square
    // hole: exterior counter-clockwise, hole clockwise.
    let exterior = square(1_000.0, 1_000.0, 4_000.0);
    let mut hole = square(2_000.0, 2_000.0, 2_000.0);
    hole.reverse();

    let total = clipped_area(&exterior, CELL) + clipped_area(&hole, CELL);
    approx(total, 16_000_000.0 - 4_000_000.0);
}

#[test]
fn a_degenerate_ring_contributes_nothing() {
    // Fewer than three points encloses no area. Real data contains these.
    approx(clipped_area(&[], CELL), 0.0);
    approx(clipped_area(&[[1.0, 1.0]], CELL), 0.0);
    approx(clipped_area(&[[1.0, 1.0], [2.0, 2.0]], CELL), 0.0);
}

#[test]
fn a_closed_ring_and_an_open_one_agree() {
    // Some formats repeat the first point at the end, some do not. Both must give
    // the same answer, or the source format would change an assessment.
    let open = square(1_000.0, 1_000.0, 2_000.0);
    let mut closed = open.clone();
    closed.push(open[0]);

    approx(clipped_area(&open, CELL), clipped_area(&closed, CELL));
}

#[test]
fn clipping_conserves_area_across_the_cells_a_feature_spans() {
    // The invariant the whole AOO computation rests on: summing a feature's
    // contribution over every cell it touches recovers its total area exactly.
    // If this drifts, per-cell fractions are wrong and so is the 1% exclusion.
    let feature = square(-5_000.0, -5_000.0, 30_000.0);
    let expected = 30_000.0 * 30_000.0;

    let mut total = 0.0;
    for col in -1..=3 {
        for row in -1..=3 {
            let min_x = f64::from(col) * 10_000.0;
            let min_y = f64::from(row) * 10_000.0;
            total += clipped_area(&feature, [min_x, min_y, min_x + 10_000.0, min_y + 10_000.0]);
        }
    }

    approx(total, expected);
}
