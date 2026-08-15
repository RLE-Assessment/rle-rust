//! The C ABI runner for the cross-language conformance corpus.
//!
//! Exercises the same `extern "C"` entry points a Julia `ccall` would reach,
//! including the allocate/free round trip, so a leak or a mis-shaped JSON
//! contract shows up here rather than in a downstream language.

// Testing a C ABI means calling it the way C does. The workspace denies unsafe
// everywhere except the capi crate and this, its test.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString};

use iucn_rle::{iucn_rle_criterion_b, iucn_rle_string_free, iucn_rle_thresholds_toml};
use serde::Deserialize;

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Case {
    id: String,
    eoo_km2: Option<f64>,
    eoo_lower_km2: Option<f64>,
    eoo_upper_km2: Option<f64>,
    aoo_cells: Option<f64>,
    clauses: serde_json::Value,
    locations: Option<u32>,
    no_plausible_threats: bool,
    locations_insufficient_information: bool,
    expect: std::collections::HashMap<String, String>,
}

/// Call the C ABI exactly as a foreign caller would, and free the result.
fn call(args: &serde_json::Value) -> serde_json::Value {
    let input = CString::new(args.to_string()).expect("no interior NUL");

    // SAFETY: `input` is a valid NUL-terminated string alive across the call,
    // and the returned pointer is freed below exactly once.
    let raw = unsafe { iucn_rle_criterion_b(input.as_ptr()) };
    assert!(!raw.is_null(), "C ABI returned NULL");

    let text = unsafe { CStr::from_ptr(raw) }
        .to_str()
        .expect("valid UTF-8")
        .to_owned();
    unsafe { iucn_rle_string_free(raw) };

    serde_json::from_str(&text).expect("valid JSON")
}

fn corpus() -> Corpus {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cases/criterion_b.json");
    serde_json::from_slice(&std::fs::read(path).expect("read corpus")).expect("parse corpus")
}

#[test]
fn every_case_matches_across_the_c_abi() {
    let corpus = corpus();
    assert!(corpus.cases.len() >= 28, "corpus shrank unexpectedly");

    let mut failures = Vec::new();

    for case in &corpus.cases {
        let mut args = serde_json::Map::new();
        for (key, value) in [
            ("eoo_km2", case.eoo_km2),
            ("eoo_lower_km2", case.eoo_lower_km2),
            ("eoo_upper_km2", case.eoo_upper_km2),
            ("aoo_cells", case.aoo_cells),
        ] {
            if let Some(v) = value {
                args.insert(key.to_owned(), serde_json::json!(v));
            }
        }
        if !case.clauses.is_null() {
            args.insert("clauses".to_owned(), case.clauses.clone());
        }
        if let Some(n) = case.locations {
            args.insert("locations".to_owned(), serde_json::json!(n));
        }
        args.insert(
            "no_plausible_threats".to_owned(),
            serde_json::json!(case.no_plausible_threats),
        );
        args.insert(
            "locations_insufficient_information".to_owned(),
            serde_json::json!(case.locations_insufficient_information),
        );

        let result = call(&serde_json::Value::Object(args));

        let by_criterion: std::collections::HashMap<&str, &str> = result["criteria"]
            .as_array()
            .unwrap_or_else(|| panic!("{}: no criteria array in {result}", case.id))
            .iter()
            .map(|c| {
                (
                    c["criterion"].as_str().unwrap(),
                    c["category"].as_str().unwrap(),
                )
            })
            .collect();

        for (key, expected) in &case.expect {
            let actual = if key == "overall" {
                result["overall"].as_str().unwrap()
            } else {
                by_criterion
                    .get(key.to_uppercase().as_str())
                    .copied()
                    .unwrap_or("<missing>")
            };
            if actual != expected {
                failures.push(format!(
                    "{}: {key} expected {expected:?}, got {actual:?}",
                    case.id
                ));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n  "));
}

#[test]
fn a_rejected_subcondition_comes_back_as_a_json_error_not_a_crash() {
    // A foreign caller cannot catch a Rust panic, so bad input must always
    // return a well-formed JSON object rather than unwinding across the ABI.
    let result = call(&serde_json::json!({
        "eoo_km2": 15000.0,
        "clauses": [{ "sub": "z", "status": "met" }]
    }));

    let error = result["error"].as_str().expect("an error key");
    assert!(
        error.contains('z'),
        "error should name the bad value: {error}"
    );
    assert!(
        error.contains("a|b"),
        "error should list valid values: {error}"
    );
}

#[test]
fn clause_c_as_a_status_is_rejected_across_the_abi() {
    // (c) is a count, not a status. Accepting a boolean here is the bug M1.5 fixed,
    // and a foreign caller must get a usable message rather than a wrong category.
    let result = call(&serde_json::json!({
        "eoo_km2": 15000.0,
        "clauses": [{ "sub": "c", "status": "met" }]
    }));

    let error = result["error"].as_str().expect("an error key");
    assert!(
        error.contains("locations"),
        "error should name the fix: {error}"
    );
}

#[test]
fn malformed_json_is_reported_rather_than_panicking() {
    let input = CString::new("{ not json").unwrap();
    // SAFETY: valid NUL-terminated string; result freed exactly once.
    let raw = unsafe { iucn_rle_criterion_b(input.as_ptr()) };
    assert!(!raw.is_null());
    let text = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_owned();
    unsafe { iucn_rle_string_free(raw) };

    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(value["error"]
        .as_str()
        .unwrap()
        .contains("invalid JSON arguments"));
}

#[test]
fn freeing_null_is_a_no_op() {
    // Julia wrappers commonly free unconditionally; this must not crash.
    // SAFETY: NULL is explicitly permitted by the contract.
    unsafe { iucn_rle_string_free(std::ptr::null_mut()) };
}

#[test]
fn thresholds_toml_crosses_the_abi_intact() {
    let raw = iucn_rle_thresholds_toml();
    assert!(!raw.is_null());
    // SAFETY: returned by this library, freed exactly once below.
    let text = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_owned();
    unsafe { iucn_rle_string_free(raw) };

    assert!(text.contains(r#"guidelines_version = "2.0""#));
}
