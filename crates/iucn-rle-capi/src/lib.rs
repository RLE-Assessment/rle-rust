//! A plain C ABI for the IUCN RLE calculation engine.
//!
//! Julia's mainstream path to Rust is `ccall` against a C dynamic library, so
//! this is what a Julia package binds to. It doubles as the escape hatch for any
//! future language, and as the strictest check that the core has not grown
//! anything exotic: if this compiles, every future binding is tractable.
//!
//! # Contract
//!
//! Arguments and results both cross as JSON encoded in NUL-terminated UTF-8.
//! That keeps the ABI to three functions instead of one per parameter, and means
//! adding a field never breaks a caller's `ccall` signature.
//!
//! Every returned pointer is owned by the caller and **must** be released with
//! [`iucn_rle_string_free`]. A NULL return means the input was not valid UTF-8 or
//! contained an interior NUL; anything else, including a rejected sub-condition,
//! comes back as a JSON object with an `error` key.
//!
//! ```julia
//! # Julia
//! ptr = ccall((:iucn_rle_criterion_b, "libiucn_rle"), Cstring, (Cstring,),
//!             """{"eoo_km2": 15000}""")
//! result = unsafe_string(ptr)
//! ccall((:iucn_rle_string_free, "libiucn_rle"), Cvoid, (Cstring,), ptr)
//! ```

// A C ABI cannot be expressed without raw pointers. This is the one crate in the
// workspace where that is true; the workspace default denies unsafe everywhere else.
#![allow(unsafe_code)]

use std::ffi::{c_char, CStr, CString};

use iucn_rle_core::ffi::{
    criterion_b_from_parts, distribution_metrics, MetricInput, PolygonInput, SubconditionInput,
    SubconditionsInput,
};
use serde::Deserialize;

/// Arguments to [`iucn_rle_criterion_b`], as JSON.
#[derive(Default, Deserialize)]
#[serde(default)]
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

/// Encode a JSON error object. Infallible: the message is escaped by serde.
fn error_json(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

/// Move a Rust string to the caller, who must free it.
fn into_c_string(s: String) -> *mut c_char {
    // A NUL can only appear here if a category or message contained one, which
    // cannot happen; returning NULL is the honest fallback rather than panicking
    // across an FFI boundary.
    CString::new(s).map_or(std::ptr::null_mut(), CString::into_raw)
}

/// The engine version. The returned pointer is static and must **not** be freed.
#[no_mangle]
pub extern "C" fn iucn_rle_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Assess IUCN RLE Criterion B from a JSON argument object.
///
/// Returns a JSON summary, or a JSON object with an `error` key. The caller owns
/// the result and must release it with [`iucn_rle_string_free`]. Returns NULL
/// only if `args_json` is NULL or not valid UTF-8.
///
/// # Safety
///
/// `args_json` must be NULL or a valid NUL-terminated C string that stays alive
/// for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn iucn_rle_criterion_b(args_json: *const c_char) -> *mut c_char {
    if args_json.is_null() {
        return into_c_string(error_json("args_json was NULL"));
    }

    // SAFETY: the caller contract above guarantees a valid NUL-terminated string.
    let Ok(text) = unsafe { CStr::from_ptr(args_json) }.to_str() else {
        return std::ptr::null_mut();
    };

    let args: Args = match serde_json::from_str(text) {
        Ok(args) => args,
        Err(e) => return into_c_string(error_json(&format!("invalid JSON arguments: {e}"))),
    };

    let subs = SubconditionsInput {
        clauses: args.clauses,
        locations: args.locations,
        no_plausible_threats: args.no_plausible_threats,
        locations_insufficient_information: args.locations_insufficient_information,
    };

    let summary = match criterion_b_from_parts(
        metric(args.eoo_km2, args.eoo_lower_km2, args.eoo_upper_km2),
        metric(args.aoo_cells, args.aoo_lower_cells, args.aoo_upper_cells),
        subs,
    ) {
        Ok(summary) => summary,
        Err(e) => return into_c_string(error_json(&e)),
    };

    match serde_json::to_string(&summary) {
        Ok(json) => into_c_string(json),
        Err(e) => into_c_string(error_json(&format!("could not serialise summary: {e}"))),
    }
}

/// The threshold table as TOML. The caller must free the result.
#[no_mangle]
pub extern "C" fn iucn_rle_thresholds_toml() -> *mut c_char {
    into_c_string(iucn_rle_core::thresholds::V2_2024_TOML.to_owned())
}

/// Release a string returned by this library.
///
/// Passing NULL is a no-op. Passing anything this library did not return, or the
/// same pointer twice, is undefined behaviour.
///
/// # Safety
///
/// `s` must be NULL or a pointer previously returned by one of this library's
/// allocating functions, and must not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn iucn_rle_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: the caller contract guarantees this came from CString::into_raw.
    drop(unsafe { CString::from_raw(s) });
}

/// Compute Criterion B spatial metrics from a distribution map, as JSON.
///
/// `polygons_json` is a JSON array of `{"ecosystem": ..., "rings": [[[lon, lat], ...]]}`.
/// Returns a JSON summary, or a JSON object with an `error` key. The caller owns the
/// result and must release it with [`iucn_rle_string_free`].
///
/// # Safety
///
/// `polygons_json` must be NULL or a valid NUL-terminated C string that stays alive
/// for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn iucn_rle_distribution_metrics(
    polygons_json: *const c_char,
) -> *mut c_char {
    if polygons_json.is_null() {
        return into_c_string(error_json("polygons_json was NULL"));
    }

    // SAFETY: the caller contract above guarantees a valid NUL-terminated string.
    let Ok(text) = (unsafe { CStr::from_ptr(polygons_json) }).to_str() else {
        return std::ptr::null_mut();
    };

    let polygons: Vec<PolygonInput> = match serde_json::from_str(text) {
        Ok(polygons) => polygons,
        Err(e) => return into_c_string(error_json(&format!("invalid JSON polygons: {e}"))),
    };

    match distribution_metrics(&polygons) {
        Ok(summary) => match serde_json::to_string(&summary) {
            Ok(json) => into_c_string(json),
            Err(e) => into_c_string(error_json(&format!("could not serialise metrics: {e}"))),
        },
        Err(e) => into_c_string(error_json(&e)),
    }
}
