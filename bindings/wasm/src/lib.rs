//! WebAssembly bindings, published as `@rle-assessment/iucn-rle`.
//!
//! Pure functions are exported **synchronously**, so the browser API for anything that
//! does not fetch is an ordinary function call rather than a Promise. Only functions
//! that take a URL become async, and only here — see `iucn-rle-engine`.
//!
//! This crate is also where the ESRI:54034 transform will be exported directly, so the
//! deck.gl viewer stops needing a hand-copied proj4 definition to work around
//! `@developmentseed/geotiff` throwing "Unsupported coordinate transformation type: 28".

use wasm_bindgen::prelude::*;

/// Version of the underlying `iucn-rle-core` calculation engine.
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    iucn_rle_core::version().to_owned()
}
