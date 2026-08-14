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

#[derive(Deserialize)]
struct Case {
    id: String,
    eoo_km2: Option<f64>,
    eoo_lower_km2: Option<f64>,
    eoo_upper_km2: Option<f64>,
    aoo_cells: Option<f64>,
    subconditions: serde_json::Value,
    expect: Expect,
}

#[derive(Deserialize)]
struct Expect {
    b1: String,
    b2: String,
    overall: String,
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
    assert!(corpus.cases.len() >= 20, "corpus shrank unexpectedly");

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
        args.insert("subconditions".to_owned(), case.subconditions.clone());

        let result = call(&serde_json::Value::Object(args));

        let by_criterion: std::collections::HashMap<&str, &str> = result["criteria"]
            .as_array()
            .expect("criteria array")
            .iter()
            .map(|c| {
                (
                    c["criterion"].as_str().unwrap(),
                    c["category"].as_str().unwrap(),
                )
            })
            .collect();

        let actual = (
            by_criterion["B1"],
            by_criterion["B2"],
            result["overall"].as_str().unwrap(),
        );
        let expected = (
            case.expect.b1.as_str(),
            case.expect.b2.as_str(),
            case.expect.overall.as_str(),
        );

        if actual != expected {
            failures.push(format!(
                "{}: expected {expected:?}, got {actual:?}",
                case.id
            ));
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
        "subconditions": [{ "sub": "z", "status": "met" }]
    }));

    let error = result["error"].as_str().expect("an error key");
    assert!(
        error.contains('z'),
        "error should name the bad value: {error}"
    );
    assert!(
        error.contains("a|b|c"),
        "error should list valid values: {error}"
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
