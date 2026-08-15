//! The distribution entry point every language binding calls.

use iucn_rle_core::ffi::{distribution_metrics, PolygonInput};

/// A counter-clockwise square in degrees.
fn square_deg(min_lon: f64, min_lat: f64, size: f64) -> Vec<Vec<[f64; 2]>> {
    vec![vec![
        [min_lon, min_lat],
        [min_lon + size, min_lat],
        [min_lon + size, min_lat + size],
        [min_lon, min_lat + size],
    ]]
}

fn polygon(ecosystem: &str, rings: Vec<Vec<[f64; 2]>>) -> PolygonInput {
    PolygonInput {
        ecosystem: ecosystem.to_owned(),
        rings,
    }
}

#[test]
fn metrics_are_returned_per_ecosystem() {
    let summary = distribution_metrics(&[
        polygon("T1.1.1", square_deg(0.0, 0.0, 1.0)),
        polygon("T6.5.1", square_deg(50.0, 0.0, 0.1)),
    ])
    .unwrap();

    assert_eq!(summary.ecosystems.len(), 2);
    let codes: Vec<&str> = summary
        .ecosystems
        .iter()
        .map(|e| e.ecosystem.as_str())
        .collect();
    assert!(codes.contains(&"T1.1.1"));
    assert!(codes.contains(&"T6.5.1"));
}

#[test]
fn both_criterion_b_metrics_are_present() {
    let summary = distribution_metrics(&[polygon("forest", square_deg(0.0, 0.0, 1.0))]).unwrap();
    let metrics = &summary.ecosystems[0];

    assert!(metrics.eoo_km2 > 12_000.0, "EOO {}", metrics.eoo_km2);
    assert!(metrics.aoo_cells > 100, "AOO {}", metrics.aoo_cells);
    assert!(metrics.occupied_cell_count >= metrics.aoo_cells);
}

#[test]
fn ecosystems_are_returned_in_a_stable_order() {
    // Sorted, not hash order: bindings serialise this and assessments diff it.
    let summary = distribution_metrics(&[
        polygon("zebra", square_deg(0.0, 0.0, 0.1)),
        polygon("alpha", square_deg(1.0, 0.0, 0.1)),
        polygon("middle", square_deg(2.0, 0.0, 0.1)),
    ])
    .unwrap();

    let codes: Vec<&str> = summary
        .ecosystems
        .iter()
        .map(|e| e.ecosystem.as_str())
        .collect();
    assert_eq!(codes, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn the_projection_is_reported_for_provenance() {
    // A cell count is meaningless without knowing which grid produced it.
    let summary = distribution_metrics(&[polygon("forest", square_deg(0.0, 0.0, 0.1))]).unwrap();
    assert_eq!(summary.grid_crs, "ESRI:54034");
    assert!((summary.cell_size_m - 10_000.0).abs() < f64::EPSILON);
}

#[test]
fn overlapping_features_are_surfaced() {
    let summary = distribution_metrics(&[
        polygon("forest", square_deg(0.0, 0.0, 1.0)),
        polygon("forest", square_deg(0.0, 0.0, 1.0)),
    ])
    .unwrap();

    assert!(
        summary.overfull_cells > 0,
        "overlapping input must be reported, not silently absorbed"
    );
}

#[test]
fn a_knife_edge_exclusion_is_flagged_per_ecosystem() {
    // Callers need to know when an AOO count is not robust, since that is exactly
    // when it might disagree with another implementation.
    let summary = distribution_metrics(&[polygon("forest", square_deg(0.0, 0.0, 1.0))]).unwrap();
    assert!(!summary.ecosystems[0].aoo_near_boundary);
}

#[test]
fn no_polygons_is_not_an_error() {
    let summary = distribution_metrics(&[]).unwrap();
    assert!(summary.ecosystems.is_empty());
}

#[test]
fn an_empty_ring_list_is_rejected_with_a_clear_message() {
    // Distinguishable from "no polygons": the caller supplied a feature but no
    // geometry, which is a malformed input rather than an empty dataset.
    let err = distribution_metrics(&[polygon("forest", vec![])]).unwrap_err();
    assert!(
        err.contains("forest"),
        "error should name the feature: {err}"
    );
    assert!(
        err.contains("ring"),
        "error should say what is missing: {err}"
    );
}

#[test]
fn out_of_range_coordinates_are_rejected() {
    // A latitude of 200 is not a rounding artefact; it means the coordinates are in
    // the wrong order or the wrong CRS. Silently clamping would produce a
    // plausible-looking but wrong AOO.
    let err = distribution_metrics(&[polygon("forest", vec![vec![[10.0, 200.0], [11.0, 201.0]]])])
        .unwrap_err();
    assert!(err.contains("latitude"), "{err}");
}

#[test]
fn longitudes_beyond_the_antimeridian_are_rejected() {
    let err = distribution_metrics(&[polygon("forest", vec![vec![[400.0, 10.0], [401.0, 11.0]]])])
        .unwrap_err();
    assert!(err.contains("longitude"), "{err}");
}
