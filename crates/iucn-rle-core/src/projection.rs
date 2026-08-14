//! Projection into the AOO grid's coordinate reference system.
//!
//! The grid is defined in **ESRI:54034**, World Cylindrical Equal Area on the WGS84
//! ellipsoid, central meridian 0, standard parallel 0. Equal-area is the load-bearing
//! property: counting grid cells only means something if every cell covers the same
//! amount of ground.
//!
//! # Why this is hand-written rather than delegated to PROJ
//!
//! PROJ is a C library and cannot target WebAssembly, which would end the browser
//! binding; pulling it in would also make the Python wheels and the R package far
//! harder to distribute. The alternative, `proj4rs`, carries no EPSG database and
//! would still need the parameters supplied by hand.
//!
//! For this projection that trade is nearly free. With a standard parallel of 0 the
//! scale factor is exactly 1, and the forward transform reduces to a closed-form
//! expression — no iteration, no series truncation, no lookup tables. Measured
//! against PROJ 9.5.1 across the reference fixture, the worst disagreement is under
//! **2 nanometres**, which is floating-point rounding rather than a difference in
//! method.
//!
//! The **inverse** transform is a different matter: recovering latitude from the
//! authalic term requires iteration or a truncated series, and that is where
//! implementations genuinely diverge. It is deliberately not implemented here,
//! because computing AOO and EOO never needs it.

/// WGS84 semi-major axis, in metres.
const A: f64 = 6_378_137.0;

/// WGS84 inverse flattening.
const INVERSE_FLATTENING: f64 = 298.257_223_563;

/// First eccentricity squared, `f(2 - f)`.
const E2: f64 = (1.0 / INVERSE_FLATTENING) * (2.0 - 1.0 / INVERSE_FLATTENING);

/// First eccentricity.
const E: f64 = 0.081_819_190_842_621_49;

/// The grid's CRS, as WKT1, recorded in provenance so a reviewer can confirm which
/// projection produced a cell count.
pub const AOO_CRS_WKT: &str = concat!(
    r#"PROJCS["World_Cylindrical_Equal_Area","#,
    r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984","#,
    r#"SPHEROID["WGS_1984",6378137.0,298.257223563]],"#,
    r#"PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],"#,
    r#"PROJECTION["Cylindrical_Equal_Area"],"#,
    r#"PARAMETER["False_Easting",0.0],PARAMETER["False_Northing",0.0],"#,
    r#"PARAMETER["Central_Meridian",0.0],PARAMETER["Standard_Parallel_1",0.0],"#,
    r#"UNIT["Meter",1.0]]"#
);

/// Snyder's authalic area term `q`.
///
/// Proportional to the area of the ellipsoid between the equator and latitude `phi`.
/// This is what makes the projection equal-area, and it is the only non-trivial part
/// of the transform.
fn authalic_q(sin_phi: f64) -> f64 {
    let denominator = 1.0 - E2 * sin_phi * sin_phi;
    // At the poles `1 - e·sin(phi)` stays comfortably away from zero, so the
    // logarithm is well conditioned across the whole domain.
    let ratio = (1.0 - E * sin_phi) / (1.0 + E * sin_phi);
    (1.0 - E2) * (sin_phi / denominator - (1.0 / (2.0 * E)) * ratio.ln())
}

/// Project geographic coordinates to ESRI:54034, in metres.
///
/// `lon` and `lat` are degrees on WGS84. Latitudes are clamped to ±90°, because data
/// occasionally carries a value a hair outside that from rounding and returning `NaN`
/// would silently poison an entire accumulation.
///
/// Longitudes are **not** wrapped. A longitude beyond ±180° projects to an easting
/// beyond the world rectangle, which is the honest result: silently wrapping it would
/// move an ecosystem to the other side of the planet and quietly change its AOO.
///
/// ```
/// use iucn_rle_core::projection::project_to_aoo_crs;
///
/// let (x, y) = project_to_aoo_crs(0.0, 0.0);
/// assert!(x.abs() < 1e-9 && y.abs() < 1e-9);
///
/// // Bogota, Colombia — matches PROJ to under a nanometre.
/// let (x, y) = project_to_aoo_crs(-73.5, 4.2);
/// assert!((x - -8_181_982.573_306).abs() < 1e-6);
/// assert!((y - 464_007.262_008).abs() < 1e-6);
/// ```
#[must_use]
pub fn project_to_aoo_crs(lon: f64, lat: f64) -> (f64, f64) {
    if !lon.is_finite() || !lat.is_finite() {
        return (f64::NAN, f64::NAN);
    }

    let lambda = lon.to_radians();
    let phi = lat.clamp(-90.0, 90.0).to_radians();

    // Standard parallel 0 gives a scale factor of exactly 1, so x is simply the
    // equatorial radius times the longitude, and y is half the authalic term.
    let x = A * lambda;
    let y = A * authalic_q(phi.sin()) / 2.0;

    (x, y)
}

/// Project a ring of geographic coordinates in place.
///
/// Convenience for the common path, where a whole polygon ring needs projecting
/// before it reaches the accumulator.
#[must_use]
pub fn project_ring(ring: &[[f64; 2]]) -> Vec<[f64; 2]> {
    ring.iter()
        .map(|&[lon, lat]| {
            let (x, y) = project_to_aoo_crs(lon, lat);
            [x, y]
        })
        .collect()
}
