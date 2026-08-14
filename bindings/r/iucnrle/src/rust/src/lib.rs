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
use iucn_rle_core::ffi::{criterion_b_from_parts, MetricInput, SubconditionInput};

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
/// Sub-conditions arrive as two parallel character vectors so the interface
/// stays a plain R vector pair rather than a list of lists.
/// @export
#[extendr]
fn rle_criterion_b_json(
    eoo_km2: Nullable<f64>,
    aoo_cells: Nullable<f64>,
    sub_names: Vec<String>,
    sub_statuses: Vec<String>,
    eoo_lower_km2: Nullable<f64>,
    eoo_upper_km2: Nullable<f64>,
    aoo_lower_cells: Nullable<f64>,
    aoo_upper_cells: Nullable<f64>,
) -> Result<String, Error> {
    if sub_names.len() != sub_statuses.len() {
        return Err(Error::Other(
            "sub-condition names and statuses must have the same length".into(),
        ));
    }

    let subs: Vec<SubconditionInput> = sub_names
        .into_iter()
        .zip(sub_statuses)
        .map(|(sub, status)| SubconditionInput { sub, status })
        .collect();

    // Everything below is plain data; no SEXP is touched until this returns.
    let summary = criterion_b_from_parts(
        metric(eoo_km2, eoo_lower_km2, eoo_upper_km2),
        metric(aoo_cells, aoo_lower_cells, aoo_upper_cells),
        &subs,
    )
    .map_err(Error::Other)?;

    serde_json::to_string(&summary).map_err(|e| Error::Other(e.to_string()))
}

// Registers the exported functions with R. See the matching C code in entrypoint.c.
extendr_module! {
    mod iucnrle;
    fn rle_version;
    fn rle_thresholds_toml;
    fn rle_thresholds_sha256;
    fn rle_criterion_b_json;
}
