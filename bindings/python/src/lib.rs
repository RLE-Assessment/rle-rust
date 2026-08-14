//! Python bindings, exposed to users as the `iucn_rle` package.
//!
//! Deliberately synchronous. We do not bridge Rust futures into asyncio: the
//! `pyo3-async-runtimes` entry point requires `Send` futures, and its `!Send`
//! variant has been deprecated since 0.18.0. Adopting either would push `Send`
//! bounds back through the engine and break the WASM binding, which is the
//! harder constraint.
//!
//! Instead, every long-running function here must release the GIL with
//! `Python::detach` around the whole computation. That is a contract, not an
//! optimisation: it is what lets notebook users stay responsive with
//! `await asyncio.to_thread(...)`, which matters because Quarto and Jupyter
//! kernels already run an asyncio event loop that a naive blocking call freezes.
//!
//! Results cross as JSON and are turned into dicts by the Python wrapper. The
//! payload is tens of values, so the serialisation cost is irrelevant, and it
//! keeps one flattening (`iucn_rle_core::Summary`) shared by all five bindings.

use iucn_rle_core::ffi::{
    criterion_b_from_parts, MetricInput, SubconditionInput, SubconditionsInput,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Version of the underlying `iucn-rle-core` calculation engine.
#[pyfunction]
fn version() -> &'static str {
    iucn_rle_core::version()
}

/// The threshold table as TOML, for callers that want to audit the numbers.
#[pyfunction]
fn thresholds_toml() -> &'static str {
    iucn_rle_core::thresholds::V2_2024_TOML
}

/// SHA-256 of the threshold table.
#[pyfunction]
fn thresholds_sha256() -> &'static str {
    iucn_rle_core::thresholds::V2_2024_SHA256
}

fn metric(best: Option<f64>, lower: Option<f64>, upper: Option<f64>) -> Option<MetricInput> {
    best.map(|best| match (lower, upper) {
        (Some(lower), Some(upper)) => MetricInput::bounded(best, lower, upper),
        _ => MetricInput::point(best),
    })
}

/// Assess IUCN RLE Criterion B, returning a JSON summary.
///
/// The Python wrapper parses this into a dict; see `iucn_rle/__init__.py`.
#[pyfunction]
#[pyo3(signature = (
    eoo_km2 = None,
    aoo_cells = None,
    clauses = None,
    locations = None,
    no_plausible_threats = false,
    locations_insufficient_information = false,
    eoo_lower_km2 = None,
    eoo_upper_km2 = None,
    aoo_lower_cells = None,
    aoo_upper_cells = None,
))]
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn criterion_b_json(
    py: Python<'_>,
    eoo_km2: Option<f64>,
    aoo_cells: Option<f64>,
    clauses: Option<Vec<(String, String)>>,
    locations: Option<u32>,
    no_plausible_threats: bool,
    locations_insufficient_information: bool,
    eoo_lower_km2: Option<f64>,
    eoo_upper_km2: Option<f64>,
    aoo_lower_cells: Option<f64>,
    aoo_upper_cells: Option<f64>,
) -> PyResult<String> {
    let subs = SubconditionsInput {
        clauses: clauses
            .unwrap_or_default()
            .into_iter()
            .map(|(sub, status)| SubconditionInput { sub, status })
            .collect(),
        locations,
        no_plausible_threats,
        locations_insufficient_information,
    };

    let eoo = metric(eoo_km2, eoo_lower_km2, eoo_upper_km2);
    let aoo = metric(aoo_cells, aoo_lower_cells, aoo_upper_cells);

    // Release the GIL for the computation. Trivial for Criterion B, but the
    // contract must hold from the first function so callers can rely on
    // `asyncio.to_thread` working once the I/O paths land.
    let summary = py
        .detach(|| criterion_b_from_parts(eoo, aoo, subs))
        .map_err(PyValueError::new_err)?;

    serde_json::to_string(&summary)
        .map_err(|e| PyValueError::new_err(format!("could not serialise summary: {e}")))
}

#[pymodule]
fn _iucn_rle(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(thresholds_toml, m)?)?;
    m.add_function(wrap_pyfunction!(thresholds_sha256, m)?)?;
    m.add_function(wrap_pyfunction!(criterion_b_json, m)?)?;
    m.add("__version__", iucn_rle_core::version())?;
    Ok(())
}
