//! R bindings for the IUCN Red List of Ecosystems calculation engine.
//!
//! R's C API is single-threaded and not reentrant, so nothing inside a future may
//! touch an SEXP. Once async I/O arrives (M3), the rule is: run the future to
//! completion first, then build the R object from the returned plain data.

use extendr_api::prelude::*;

/// Version of the underlying iucn-rle-core calculation engine.
/// @export
#[extendr]
fn rle_version() -> String {
    iucn_rle_core::version().to_owned()
}

// Registers the exported functions with R. See the matching C code in entrypoint.c.
extendr_module! {
    mod iucnrle;
    fn rle_version;
}
