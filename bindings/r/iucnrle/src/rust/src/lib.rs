//! R bindings for the IUCN Red List of Ecosystems calculation engine.
//!
//! R's C API is single-threaded and not reentrant, so nothing inside a future
//! may touch an SEXP. Once async I/O arrives (M3), the rule is: run the future
//! to completion first, then build the R object from the returned plain data.
//!
//! Results cross as a JSON string and are turned into an R list by the wrapper
//! in `R/iucnrle.R`. The payload is tens of values, so the cost is irrelevant,
//! and it keeps one flattening shared with the Python, WASM, and C bindings.

use extendr_api::prelude::*;
use iucn_rle_core::ffi::{
    criterion_b_from_parts, distribution_metrics, MetricInput, PolygonInput, SubconditionInput,
    SubconditionsInput,
};

/// Version of the underlying iucn-rle-core calculation engine.
/// @export
#[extendr]
fn rle_version() -> String {
    iucn_rle_core::version().to_owned()
}

/// The IUCN threshold table as TOML.
/// @export
#[extendr]
fn rle_thresholds_toml() -> String {
    iucn_rle_core::thresholds::V2_2024_TOML.to_owned()
}

/// SHA-256 of the IUCN threshold table.
/// @export
#[extendr]
fn rle_thresholds_sha256() -> String {
    iucn_rle_core::thresholds::V2_2024_SHA256.to_owned()
}

fn optional(value: Nullable<f64>) -> Option<f64> {
    match value {
        Nullable::NotNull(v) if v.is_finite() => Some(v),
        _ => None,
    }
}

fn metric(best: Nullable<f64>, lower: Nullable<f64>, upper: Nullable<f64>) -> Option<MetricInput> {
    optional(best).map(|best| match (optional(lower), optional(upper)) {
        (Some(lower), Some(upper)) => MetricInput::bounded(best, lower, upper),
        _ => MetricInput::point(best),
    })
}

/// Assess IUCN RLE Criterion B, returning a JSON summary.
///
/// Clauses arrive as two parallel character vectors so the interface stays a plain
/// R vector pair rather than a list of lists. Clause (c) is separate because it is a
/// count of threat-defined locations, not a status.
/// @export
#[extendr]
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn rle_criterion_b_json(
    eoo_km2: Nullable<f64>,
    aoo_cells: Nullable<f64>,
    clause_names: Vec<String>,
    clause_statuses: Vec<String>,
    locations: Nullable<f64>,
    no_plausible_threats: bool,
    locations_insufficient_information: bool,
    eoo_lower_km2: Nullable<f64>,
    eoo_upper_km2: Nullable<f64>,
    aoo_lower_cells: Nullable<f64>,
    aoo_upper_cells: Nullable<f64>,
) -> Result<String, Error> {
    if clause_names.len() != clause_statuses.len() {
        return Err(Error::Other(
            "clause names and statuses must have the same length".into(),
        ));
    }

    // R has no integer-only numeric literal, so a count arrives as a double.
    let locations = match optional(locations) {
        Some(n) if n >= 0.0 => Some(n as u32),
        Some(n) => {
            return Err(Error::Other(format!(
                "threat-defined locations must not be negative, got {n}"
            )))
        }
        None => None,
    };

    let subs = SubconditionsInput {
        clauses: clause_names
            .into_iter()
            .zip(clause_statuses)
            .map(|(sub, status)| SubconditionInput { sub, status })
            .collect(),
        locations,
        no_plausible_threats,
        locations_insufficient_information,
    };

    // Everything below is plain data; no SEXP is touched until this returns.
    let summary = criterion_b_from_parts(
        metric(eoo_km2, eoo_lower_km2, eoo_upper_km2),
        metric(aoo_cells, aoo_lower_cells, aoo_upper_cells),
        subs,
    )
    .map_err(Error::Other)?;

    serde_json::to_string(&summary).map_err(|e| Error::Other(e.to_string()))
}

/// Compute Criterion B spatial metrics from a distribution map, returning JSON.
///
/// Polygons arrive as a JSON array rather than as nested R lists. Walking a deeply
/// nested SEXP structure across the FFI boundary would be far more code for no gain,
/// and jsonlite is already a dependency of the R wrapper.
/// @export
#[extendr]
fn rle_distribution_metrics_json(polygons_json: &str) -> Result<String, Error> {
    let polygons: Vec<PolygonInput> = serde_json::from_str(polygons_json)
        .map_err(|e| Error::Other(format!("invalid polygons: {e}")))?;

    // Plain data throughout; no SEXP is touched until this returns.
    let summary = distribution_metrics(&polygons).map_err(Error::Other)?;

    serde_json::to_string(&summary).map_err(|e| Error::Other(e.to_string()))
}

// Registers the exported functions with R. See the matching C code in entrypoint.c.
extendr_module! {
    mod iucnrle;
    fn rle_version;
    fn rle_thresholds_toml;
    fn rle_thresholds_sha256;
    fn rle_criterion_b_json;
    fn rle_distribution_metrics_json;
}
