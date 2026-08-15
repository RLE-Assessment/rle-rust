//! Projecting geographic coordinates into the AOO grid's CRS.
//!
//! The AOO grid is defined in ESRI:54034, World Cylindrical Equal Area on the WGS84
//! ellipsoid with a standard parallel of 0. Equal-area is the load-bearing property:
//! a count of grid cells only means anything if every cell covers the same ground
//! area.
//!
//! Reference values come from PROJ, via `tools/generate_projection_fixture.py`.

use std::path::PathBuf;

use iucn_rle_core::projection::{project_to_aoo_crs, AOO_CRS_WKT};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    tolerance_m: f64,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    why: String,
    lon: f64,
    lat: f64,
    x: f64,
    y: f64,
}

fn fixture() -> Fixture {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cases/projection.json");
    serde_json::from_slice(&std::fs::read(path).expect("read projection fixture"))
        .expect("parse projection fixture")
}

#[test]
fn matches_proj_at_every_reference_point() {
    let fixture = fixture();
    assert!(fixture.cases.len() >= 20, "fixture shrank unexpectedly");

    let mut worst: f64 = 0.0;
    let mut failures = Vec::new();

    for case in &fixture.cases {
        let (x, y) = project_to_aoo_crs(case.lon, case.lat);
        let deviation = (x - case.x).abs().max((y - case.y).abs());
        worst = worst.max(deviation);

        if deviation > fixture.tolerance_m {
            failures.push(format!(
                "{} ({}, {}): expected ({}, {}), got ({x}, {y}), off by {deviation:.3e} m",
                case.why, case.lon, case.lat, case.x, case.y
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} reference points disagree with PROJ:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );

    // Deliberately far tighter than the fixture's own 1e-6 m tolerance. The forward
    // transform is closed-form for a standard parallel of 0 — no iteration, no series
    // truncation — so agreement with PROJ should be at floating-point rounding level,
    // not merely "close enough". Measured at under 2 nanometres when written; 1e-8 m
    // leaves headroom for a different rounding order without letting an actual
    // algorithmic change slip through.
    //
    // This is the assertion that retires the project's largest risk: if the
    // projection matches PROJ to rounding, then AOO cell membership cannot diverge
    // from rle-python's for numerical reasons.
    assert!(
        worst < 1e-8,
        "worst deviation {worst:.3e} m is larger than rounding error suggests; \
         the transform may no longer be closed-form-equivalent to PROJ"
    );
}

#[test]
fn the_origin_maps_to_the_origin() {
    let (x, y) = project_to_aoo_crs(0.0, 0.0);
    assert!(x.abs() < 1e-9 && y.abs() < 1e-9);
}

#[test]
fn longitude_scales_linearly() {
    // With a standard parallel of 0 the x axis is just the equatorial radius times
    // the longitude in radians, so doubling the longitude doubles x exactly.
    let (x1, _) = project_to_aoo_crs(10.0, 40.0);
    let (x2, _) = project_to_aoo_crs(20.0, 40.0);
    assert!((x2 - 2.0 * x1).abs() < 1e-6);
}

#[test]
fn latitude_does_not_affect_easting() {
    // A cylindrical projection: meridians are straight and equally spaced, so x
    // depends only on longitude. Anything else would distort cell columns.
    let (x_equator, _) = project_to_aoo_crs(-73.5, 0.0);
    let (x_high, _) = project_to_aoo_crs(-73.5, 70.0);
    assert!((x_equator - x_high).abs() < 1e-9);
}

#[test]
fn the_projection_is_symmetric_about_the_equator() {
    let (_, north) = project_to_aoo_crs(30.0, 27.5);
    let (_, south) = project_to_aoo_crs(30.0, -27.5);
    assert!((north + south).abs() < 1e-9);
}

#[test]
fn northings_increase_monotonically_with_latitude() {
    // Required for the grid to be well ordered: a point further north must never
    // land in a lower row.
    let mut previous = f64::NEG_INFINITY;
    for step in -900..=900 {
        let lat = f64::from(step) / 10.0;
        let (_, y) = project_to_aoo_crs(0.0, lat);
        assert!(y > previous, "northing went backwards at latitude {lat}");
        previous = y;
    }
}

#[test]
fn the_world_has_the_area_of_the_ellipsoid() {
    // The defining property of an equal-area projection, checked end to end: the
    // projected world is a rectangle whose area equals the authalic surface area of
    // the WGS84 ellipsoid, about 5.100657e14 m². If this is wrong, every AOO cell
    // covers the wrong amount of ground.
    let (east, _) = project_to_aoo_crs(180.0, 0.0);
    let (_, north) = project_to_aoo_crs(0.0, 90.0);

    let projected_area = (2.0 * east) * (2.0 * north);
    let ellipsoid_area = 5.100_656_217_2e14;

    let relative_error = (projected_area - ellipsoid_area).abs() / ellipsoid_area;
    assert!(
        relative_error < 1e-9,
        "projected world area {projected_area:.6e} m2 differs from the ellipsoid's \
         {ellipsoid_area:.6e} m2 by {relative_error:.3e}"
    );
}

#[test]
fn out_of_range_latitudes_are_clamped_rather_than_producing_nonsense() {
    // Data occasionally carries latitudes a hair outside +/-90 from rounding. The
    // authalic term is undefined beyond the poles, and returning NaN would silently
    // poison an entire accumulation.
    let (_, y) = project_to_aoo_crs(0.0, 90.000_001);
    let (_, pole) = project_to_aoo_crs(0.0, 90.0);
    assert!(y.is_finite());
    assert!((y - pole).abs() < 1e-6);
}

#[test]
fn the_crs_is_identified_for_provenance() {
    // Recorded alongside results so a reviewer can confirm which projection produced
    // a cell count.
    assert!(AOO_CRS_WKT.contains("World_Cylindrical_Equal_Area"));
    assert!(AOO_CRS_WKT.contains("WGS_1984"));
}
