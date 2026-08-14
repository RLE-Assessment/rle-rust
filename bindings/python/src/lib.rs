//! Python bindings, exposed to users as the `iucn_rle` package.
//!
//! Deliberately synchronous. We do not bridge Rust futures into asyncio: the
//! `pyo3-async-runtimes` entry point requires `Send` futures, and its `!Send` variant
//! has been deprecated since 0.18.0. Adopting either would push `Send` bounds back
//! through the engine and break the WASM binding, which is the harder constraint.
//!
//! Instead, every long-running function here must release the GIL with `Python::detach`
//! around the whole fetch-and-compute. That is a contract, not an optimisation: it is
//! what lets notebook users stay responsive with
//! `await asyncio.to_thread(iucn_rle.aoo_grid, url)`, which matters because Quarto and
//! Jupyter kernels already run an asyncio event loop that a naive blocking call freezes.

use pyo3::prelude::*;

/// Version of the underlying `iucn-rle-core` calculation engine.
#[pyfunction]
fn version() -> &'static str {
    iucn_rle_core::version()
}

#[pymodule]
fn _iucn_rle(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add("__version__", iucn_rle_core::version())?;
    Ok(())
}
