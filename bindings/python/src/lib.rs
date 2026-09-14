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
    criterion_b_from_parts, distribution_metrics, MetricInput, PolygonInput, SubconditionInput,
    SubconditionsInput,
};
use iucn_rle_engine::blocking::{describe_failure, distribution_metrics_from_url};
use iucn_rle_engine::{EngineError, ReadOptions};
use iucn_rle_format::geoparquet::{Bbox, Query};
use pyo3::exceptions::{PyIOError, PyValueError};
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

/// Compute Criterion B spatial metrics from a distribution map, returning JSON.
///
/// `polygons` is a list of `(ecosystem, rings)` pairs, where `rings` is the exterior
/// ring followed by any holes, each a list of `[lon, lat]` pairs in degrees.
#[pyfunction]
fn distribution_metrics_json(
    py: Python<'_>,
    polygons: Vec<(String, Vec<Vec<[f64; 2]>>)>,
) -> PyResult<String> {
    let inputs: Vec<PolygonInput> = polygons
        .into_iter()
        .map(|(ecosystem, rings)| PolygonInput { ecosystem, rings })
        .collect();

    // Release the GIL for the whole computation. This is the path that will run for
    // minutes on a national map, so `await asyncio.to_thread(...)` has to work.
    let summary = py
        .detach(|| distribution_metrics(&inputs))
        .map_err(PyValueError::new_err)?;

    serde_json::to_string(&summary)
        .map_err(|e| PyValueError::new_err(format!("could not serialise metrics: {e}")))
}

/// Compute Criterion B spatial metrics from a remote `GeoParquet` file, returning JSON.
///
/// Reads over HTTP range requests: the footer first, then only the row groups the
/// filters could not rule out. Nothing is written to disk.
#[pyfunction]
#[pyo3(signature = (url, ecosystem_column, *, bbox = None, ecosystems = None, footer_prefetch = None))]
// Taken by value rather than as `&str` on purpose. The closure below runs with the GIL
// released, and owning the strings means it cannot possibly hold a reference into
// memory the interpreter manages while no thread is holding the GIL.
#[allow(clippy::needless_pass_by_value)]
fn distribution_metrics_from_url_json(
    py: Python<'_>,
    url: String,
    ecosystem_column: String,
    bbox: Option<(f64, f64, f64, f64)>,
    ecosystems: Option<Vec<String>>,
    footer_prefetch: Option<u64>,
) -> PyResult<String> {
    let mut query = Query::default();
    if let Some((xmin, ymin, xmax, ymax)) = bbox {
        query = query.with_bbox(Bbox::new(xmin, ymin, xmax, ymax));
    }
    if let Some(codes) = ecosystems {
        query = query.with_ecosystems(codes);
    }

    let options = footer_prefetch.map_or_else(ReadOptions::default, |footer_prefetch| {
        ReadOptions { footer_prefetch }
    });

    // Release the GIL for the entire fetch-and-decode. This is the contract the whole
    // binding rests on, not a refinement: the call is dominated by waiting on a socket,
    // and Quarto and Jupyter kernels run an asyncio event loop that stops dead for as
    // long as the GIL is held. On a national dataset that is minutes of frozen kernel.
    //
    // Nothing about the returned numbers changes either way, so this cannot be verified
    // by reading the output — see `test_the_gil_is_released_during_a_remote_read`, which
    // watches the main thread instead.
    let metrics = py
        .detach(|| distribution_metrics_from_url(&url, &ecosystem_column, &query, &options))
        .map_err(|error| remote_error(&url, &error))?;
    serde_json::to_string(&metrics)
        .map_err(|e| PyValueError::new_err(format!("could not serialise metrics: {e}")))
}

/// Turn an engine failure into the Python exception that fits it.
///
/// A transport failure is an `OSError` because that is what a Python caller catches
/// around anything networked; everything else — a missing column, a projected CRS, a
/// codec this build lacks — is a `ValueError`, since the fix is to the arguments or the
/// file rather than to the connection.
fn remote_error(url: &str, error: &EngineError) -> PyErr {
    let message = describe_failure(url, error);
    match error {
        EngineError::Io(_) => PyIOError::new_err(message),
        _ => PyValueError::new_err(message),
    }
}

#[pymodule]
fn _iucn_rle(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(thresholds_toml, m)?)?;
    m.add_function(wrap_pyfunction!(thresholds_sha256, m)?)?;
    m.add_function(wrap_pyfunction!(criterion_b_json, m)?)?;
    m.add_function(wrap_pyfunction!(distribution_metrics_json, m)?)?;
    m.add_function(wrap_pyfunction!(distribution_metrics_from_url_json, m)?)?;
    m.add("__version__", iucn_rle_core::version())?;
    Ok(())
}
