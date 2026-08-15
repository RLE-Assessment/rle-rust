//! Extent of occurrence: the Criterion B1 metric.
//!
//! IUCN (2024) Guidelines v2.0, §6.3.2, p. 67: the EOO is "the area (km²) of a
//! minimum convex polygon – the smallest polygon in which no internal angle exceeds
//! 180° that encompasses all known current spatial occurrences of the ecosystem
//! type."

use iucn_rle_core::eoo::{convex_hull, eoo_km2};

fn approx(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn the_hull_of_a_squares_corners_is_the_square() {
    let points = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let hull = convex_hull(&points);
    assert_eq!(hull.len(), 4);
}

#[test]
fn interior_points_do_not_change_the_hull() {
    // The hull depends only on the outermost occurrences. Adding detail inside must
    // not change the EOO, or the metric would reward sparse mapping.
    let corners = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let with_interior = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [0.0, 10.0],
        [5.0, 5.0],
        [2.0, 7.0],
        [8.0, 3.0],
    ];

    approx(eoo_km2(&with_interior), eoo_km2(&corners), 1e-12);
}

#[test]
fn collinear_points_on_an_edge_are_not_vertices() {
    // A vertex whose internal angle is exactly 180° is not a corner of a *minimum*
    // convex polygon, and keeping it would make hull comparisons unstable.
    let points = [
        [0.0, 0.0],
        [5.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [0.0, 10.0],
    ];
    assert_eq!(convex_hull(&points).len(), 4);
}

#[test]
fn point_order_does_not_matter() {
    let clockwise = [[0.0, 0.0], [0.0, 10.0], [10.0, 10.0], [10.0, 0.0]];
    let scattered = [[10.0, 10.0], [0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
    approx(eoo_km2(&clockwise), eoo_km2(&scattered), 1e-12);
}

#[test]
fn the_hull_spans_the_gap_between_disjunct_occurrences() {
    // The rule most often got wrong (§6.3.2, p. 67): the minimum convex polygon
    // "must not exclude any areas, discontinuities or disjunctions, regardless of
    // whether the ecosystem can occur in those areas or not."
    //
    // Two clusters 100 km apart enclose the whole 100 km span, including everything
    // between them. Clipping the hull to where the ecosystem actually occurs would
    // shrink the EOO and inflate the threat category.
    let west = [[0.0, 0.0], [10_000.0, 0.0], [0.0, 10_000.0]];
    let east = [[100_000.0, 0.0], [110_000.0, 0.0], [110_000.0, 10_000.0]];
    let both: Vec<[f64; 2]> = west.iter().chain(east.iter()).copied().collect();

    let combined = eoo_km2(&both);
    assert!(
        combined > eoo_km2(&west) + eoo_km2(&east),
        "the hull must span the gap, not just cover the clusters"
    );
    // The bounding span alone is 110 km x 10 km = 1,100 km²; the hull is a large
    // fraction of that, and far more than the clusters' own 100 km².
    assert!(combined > 500.0, "got {combined} km2");
}

#[test]
fn area_is_reported_in_square_kilometres() {
    // Coordinates are metres in an equal-area projection; the Guidelines report EOO
    // in km². A 100 km x 100 km square is 10,000 km².
    let points = [
        [0.0, 0.0],
        [100_000.0, 0.0],
        [100_000.0, 100_000.0],
        [0.0, 100_000.0],
    ];
    approx(eoo_km2(&points), 10_000.0, 1e-9);
}

#[test]
fn fewer_than_three_points_enclose_no_area() {
    // A single occurrence, or two, has no EOO. It must be zero rather than a small
    // positive number: rle-python buffers degenerate hulls by 0.0001 so its map
    // renderer has something to draw, and that fake area then flows into the B1
    // threshold and can manufacture a Critically Endangered listing.
    approx(eoo_km2(&[]), 0.0, 1e-12);
    approx(eoo_km2(&[[1.0, 1.0]]), 0.0, 1e-12);
    approx(eoo_km2(&[[1.0, 1.0], [5.0, 5.0]]), 0.0, 1e-12);
}

#[test]
fn wholly_collinear_points_enclose_no_area() {
    // A linear ecosystem — a river reach, a shoreline — mapped as a straight line
    // has a degenerate hull and zero EOO.
    let points = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [30.0, 0.0]];
    approx(eoo_km2(&points), 0.0, 1e-12);
}

#[test]
fn duplicate_points_are_harmless() {
    let points = [
        [0.0, 0.0],
        [0.0, 0.0],
        [10_000.0, 0.0],
        [10_000.0, 0.0],
        [0.0, 10_000.0],
    ];
    approx(eoo_km2(&points), 50.0, 1e-9);
}

#[test]
fn the_hull_is_counter_clockwise() {
    // Positive winding, so the area is positive and the ring composes with the
    // clipping code without a sign flip.
    let points = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let hull = convex_hull(&points);
    assert!(iucn_rle_core::geometry::ring_area(&hull) > 0.0);
}

#[test]
fn every_input_point_lies_inside_the_hull() {
    // The defining property, checked directly rather than assumed: no occurrence may
    // fall outside the polygon that is supposed to encompass all of them.
    let points: Vec<[f64; 2]> = (0..60)
        .map(|i| {
            let angle = f64::from(i) * 0.7;
            [angle.cos() * 1_000.0 + 37.0, angle.sin() * 700.0 - 12.0]
        })
        .collect();

    let hull = convex_hull(&points);

    for &[px, py] in &points {
        for i in 0..hull.len() {
            let [ax, ay] = hull[i];
            let [bx, by] = hull[(i + 1) % hull.len()];
            // Counter-clockwise hull: every point must be left of, or on, each edge.
            let cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
            assert!(
                cross >= -1e-6,
                "point [{px}, {py}] falls outside hull edge {i}"
            );
        }
    }
}
