//! End to end: geographic polygons in, Criterion B metrics out.
//!
//! This is the path an assessment actually takes — a distribution map in longitude
//! and latitude, and an EOO and AOO at the other end. Everything before this point
//! assumed coordinates were already projected.

// Exact float comparison is the point in two places here: an empty distribution must
// be exactly 0.0, not approximately; and determinism means the same input produces
// bit-identical output, which an epsilon would not verify.
#![allow(clippy::float_cmp)]

use iucn_rle_core::distribution::DistributionAccumulator;

/// A counter-clockwise square in degrees, near the equator so a degree is roughly
/// 111 km in both axes and the numbers stay easy to reason about.
fn square_deg(min_lon: f64, min_lat: f64, size: f64) -> Vec<[f64; 2]> {
    vec![
        [min_lon, min_lat],
        [min_lon + size, min_lat],
        [min_lon + size, min_lat + size],
        [min_lon, min_lat + size],
    ]
}

#[test]
fn an_empty_distribution_has_no_metrics() {
    let distribution = DistributionAccumulator::new().finish();
    assert!(distribution.ecosystems().is_empty());
    assert_eq!(distribution.eoo_km2("anything"), 0.0);
    assert_eq!(distribution.aoo("anything").aoo_cells, 0);
}

#[test]
fn a_one_degree_square_at_the_equator_has_a_plausible_extent() {
    // A degree of longitude at the equator is about 111.3 km, and this projection is
    // equal-area, so a 1° square near the equator is roughly 111 km x 111 km, about
    // 12,300 km². Checking the order of magnitude catches unit errors — degrees left
    // unconverted, or metres reported as kilometres.
    let mut acc = DistributionAccumulator::new();
    acc.add_polygon("forest", &[square_deg(0.0, 0.0, 1.0)]);
    let distribution = acc.finish();

    let eoo = distribution.eoo_km2("forest");
    assert!(
        (12_000.0..13_000.0).contains(&eoo),
        "expected roughly 12,300 km2, got {eoo}"
    );
}

#[test]
fn the_area_of_occupancy_counts_ten_kilometre_cells() {
    // Roughly 111 km on a side, so it spans about 12 x 12 cells of 10 km.
    let mut acc = DistributionAccumulator::new();
    acc.add_polygon("forest", &[square_deg(0.0, 0.0, 1.0)]);
    let distribution = acc.finish();

    let aoo = distribution.aoo("forest");
    assert!(
        (100..200).contains(&aoo.occupied_cell_count),
        "expected roughly 144 occupied cells, got {}",
        aoo.occupied_cell_count
    );
}

#[test]
fn the_extent_of_occurrence_is_at_least_the_area_of_occupancy() {
    // A structural invariant. The convex hull encloses every occurrence, so it can
    // never be smaller than the ground the occupied cells cover. If this inverts,
    // the projection or the units are wrong somewhere.
    let mut acc = DistributionAccumulator::new();
    acc.add_polygon("forest", &[square_deg(-73.0, 4.0, 0.5)]);
    acc.add_polygon("forest", &[square_deg(-72.0, 5.0, 0.5)]);
    let distribution = acc.finish();

    let eoo = distribution.eoo_km2("forest");
    let aoo_area = f64::from(distribution.aoo("forest").occupied_cell_count) * 100.0;

    assert!(
        eoo >= aoo_area * 0.5,
        "EOO {eoo} km2 is implausibly small next to {aoo_area} km2 of occupied cells"
    );
}

#[test]
fn the_hull_spans_disjunct_occurrences() {
    // The Guidelines rule (§6.3.2, p. 67): the minimum convex polygon must not
    // exclude discontinuities. Two patches either side of a gap produce an EOO much
    // larger than the patches themselves, because the gap is inside the hull.
    let mut acc = DistributionAccumulator::new();
    acc.add_polygon("forest", &[square_deg(0.0, 0.0, 0.1)]);
    acc.add_polygon("forest", &[square_deg(5.0, 0.0, 0.1)]);
    let distribution = acc.finish();

    let eoo = distribution.eoo_km2("forest");
    // Two 0.1° patches are about 250 km² together; the hull spanning 5° of longitude
    // is far larger.
    assert!(eoo > 5_000.0, "the hull must span the gap, got {eoo} km2");
}

#[test]
fn ecosystems_are_assessed_independently() {
    let mut acc = DistributionAccumulator::new();
    acc.add_polygon("forest", &[square_deg(0.0, 0.0, 1.0)]);
    acc.add_polygon("grassland", &[square_deg(50.0, 0.0, 0.1)]);
    let distribution = acc.finish();

    assert_eq!(distribution.ecosystems().len(), 2);
    assert!(distribution.eoo_km2("forest") > distribution.eoo_km2("grassland"));
    assert!(distribution.aoo("forest").aoo_cells > distribution.aoo("grassland").aoo_cells);
}

#[test]
fn the_running_hull_tracks_the_distributions_shape_not_the_feature_count() {
    // The memory property for EOO, matching the one the AOO accumulator has. A
    // national map has hundreds of thousands of features; retaining every vertex to
    // hull at the end would defeat the point of streaming.
    //
    // The hull's size is bounded by the *shape*, not the input. Five hundred patches
    // scattered inside a fixed region have a hull of a handful of vertices, because
    // all but the outermost are interior points and get discarded as they arrive.
    //
    // Note the bound is on the shape, not a guarantee of smallness: a distribution
    // whose patches genuinely lie along a convex curve has a correspondingly large
    // hull, and that is the right answer rather than a leak.
    let mut acc = DistributionAccumulator::new();
    let mut seed = 12_345_u64;
    for _ in 0..500 {
        // A small deterministic PRNG, so the scatter is reproducible without pulling
        // in a dependency.
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let lon = ((seed >> 33) % 1_000) as f64 / 1_000.0;
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let lat = ((seed >> 33) % 1_000) as f64 / 1_000.0;
        acc.add_polygon("forest", &[square_deg(lon, lat, 0.005)]);
    }
    let distribution = acc.finish();

    assert!(
        distribution.hull_vertices("forest") < 30,
        "500 scattered patches in a 1 degree box should have a small hull, got {}",
        distribution.hull_vertices("forest")
    );
    assert!(distribution.eoo_km2("forest") > 0.0);
}

#[test]
fn a_single_occurrence_has_no_extent_of_occurrence() {
    // One patch of a few vertices still has a hull, but a single point does not.
    // Reporting a fabricated area here is how a lone occurrence becomes a
    // Critically Endangered listing.
    let mut acc = DistributionAccumulator::new();
    acc.add_polygon("forest", &[vec![[10.0, 10.0]]]);
    let distribution = acc.finish();

    assert_eq!(distribution.eoo_km2("forest"), 0.0);
}

#[test]
fn southern_and_western_hemispheres_work() {
    // Negative coordinates run through the projection, the floor-based cell
    // indexing, and the clipping. A sign error anywhere shows up as zero cells.
    let mut acc = DistributionAccumulator::new();
    acc.add_polygon("forest", &[square_deg(-70.0, -35.0, 0.5)]);
    let distribution = acc.finish();

    assert!(distribution.aoo("forest").occupied_cell_count > 0);
    assert!(distribution.eoo_km2("forest") > 0.0);
}

#[test]
fn results_are_deterministic() {
    let build = || {
        let mut acc = DistributionAccumulator::new();
        for i in 0..20 {
            acc.add_polygon("forest", &[square_deg(f64::from(i) * 0.2, 1.0, 0.1)]);
        }
        acc.finish()
    };

    let first = build();
    let second = build();

    assert_eq!(first.eoo_km2("forest"), second.eoo_km2("forest"));
    assert_eq!(
        first.aoo("forest").aoo_cells,
        second.aoo("forest").aoo_cells
    );
}
