//! WebAssembly bindings, published as `@rle-assessment/iucn-rle`.
//!
//! Pure functions are exported **synchronously**, so the browser API for
//! anything that does not fetch is an ordinary function call rather than a
//! Promise. Only functions that take a URL will become async, and only here.
//!
//! This crate is also where the ESRI:54034 transform will be exported directly,
//! so the deck.gl viewer stops needing a hand-copied proj4 definition to work
//! around `@developmentseed/geotiff` throwing "Unsupported coordinate
//! transformation type: 28".

use iucn_rle_core::ffi::{
    criterion_b_from_parts, distribution_metrics, MetricInput, PolygonInput, SubconditionInput,
    SubconditionsInput,
};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

/// Version of the underlying `iucn-rle-core` calculation engine.
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    iucn_rle_core::version().to_owned()
}

/// The threshold table as TOML, for callers that want to audit the numbers.
#[wasm_bindgen(js_name = thresholdsToml)]
#[must_use]
pub fn thresholds_toml() -> String {
    iucn_rle_core::thresholds::V2_2024_TOML.to_owned()
}

/// SHA-256 of the threshold table.
#[wasm_bindgen(js_name = thresholdsSha256)]
#[must_use]
pub fn thresholds_sha256() -> String {
    iucn_rle_core::thresholds::V2_2024_SHA256.to_owned()
}

/// Arguments to [`criterion_b`], as a plain JavaScript object.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Args {
    eoo_km2: Option<f64>,
    eoo_lower_km2: Option<f64>,
    eoo_upper_km2: Option<f64>,
    aoo_cells: Option<f64>,
    aoo_lower_cells: Option<f64>,
    aoo_upper_cells: Option<f64>,
    /// Clause (a) aspects, clause (b), and B3's rapid-collapse limb.
    clauses: Vec<SubconditionInput>,
    /// Clause (c): a COUNT of threat-defined locations, not a status.
    locations: Option<u32>,
    no_plausible_threats: bool,
    locations_insufficient_information: bool,
}

fn metric(best: Option<f64>, lower: Option<f64>, upper: Option<f64>) -> Option<MetricInput> {
    best.map(|best| match (lower, upper) {
        (Some(lower), Some(upper)) => MetricInput::bounded(best, lower, upper),
        _ => MetricInput::point(best),
    })
}

/// Assess IUCN RLE Criterion B.
///
/// ```js
/// criterionB({ eooKm2: 15000 }).overall;                                   // "EN (LC-EN)"
/// criterionB({ eooKm2: 15000, clauses: [{ sub: "a", status: "met" }] }).overall; // "EN"
///
/// // Clause (c) is a COUNT, and category dependent: CR needs exactly one
/// // threat-defined location, so three qualifies only at EN.
/// criterionB({ eooKm2: 1500, locations: 3,
///              clauses: [{ sub: "a", status: "not_met" },
///                        { sub: "b", status: "not_met" }] });
/// ```
///
/// # Errors
///
/// Throws if the argument object or a clause value is malformed.
#[wasm_bindgen(js_name = criterionB)]
pub fn criterion_b(args: JsValue) -> Result<JsValue, JsError> {
    let args: Args = if args.is_undefined() || args.is_null() {
        Args::default()
    } else {
        serde_wasm_bindgen::from_value(args).map_err(|e| JsError::new(&e.to_string()))?
    };

    let subs = SubconditionsInput {
        clauses: args.clauses,
        locations: args.locations,
        no_plausible_threats: args.no_plausible_threats,
        locations_insufficient_information: args.locations_insufficient_information,
    };

    let summary = criterion_b_from_parts(
        metric(args.eoo_km2, args.eoo_lower_km2, args.eoo_upper_km2),
        metric(args.aoo_cells, args.aoo_lower_cells, args.aoo_upper_cells),
        subs,
    )
    .map_err(|e| JsError::new(&e))?;

    serde_wasm_bindgen::to_value(&summary).map_err(|e| JsError::new(&e.to_string()))
}

/// Compute Criterion B spatial metrics from a distribution map.
///
/// ```js
/// const square = [[[0, 0], [1, 0], [1, 1], [0, 1]]];
/// const result = distributionMetrics([{ ecosystem: 'T1.1.1', rings: square }]);
/// result.ecosystems[0].eoo_km2;   // about 12309
/// result.ecosystems[0].aoo_cells;
/// ```
///
/// A ring's role comes from its position, not its winding: `rings[0]` is the
/// exterior and the rest are holes, whichever way each one winds.
///
/// # Errors
///
/// Throws if a feature has no exterior ring, or a coordinate is not a valid
/// longitude/latitude. Out-of-range coordinates are rejected rather than clamped:
/// they almost always mean the values are swapped or already projected.
#[wasm_bindgen(js_name = distributionMetrics)]
pub fn distribution_metrics_js(polygons: JsValue) -> Result<JsValue, JsError> {
    let polygons: Vec<PolygonInput> =
        serde_wasm_bindgen::from_value(polygons).map_err(|e| JsError::new(&e.to_string()))?;

    let summary = distribution_metrics(&polygons).map_err(|e| JsError::new(&e))?;

    serde_wasm_bindgen::to_value(&summary).map_err(|e| JsError::new(&e.to_string()))
}
