//! The Rust runner for the cross-language conformance corpus.
//!
//! Python, R, JavaScript and the C ABI run the identical JSON file. A binding is not
//! finished until it passes this corpus, which is what makes "all surfaces agree" a
//! checkable claim rather than an aspiration.

use std::collections::HashMap;
use std::path::PathBuf;

use iucn_rle_core::ffi::{
    criterion_b_from_parts, MetricInput, SubconditionInput, SubconditionsInput,
};
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
    clauses: Vec<SubconditionInput>,
    locations: Option<u32>,
    no_plausible_threats: bool,
    locations_insufficient_information: bool,
    expect: HashMap<String, String>,
    expect_threshold: HashMap<String, String>,
}

fn corpus() -> Corpus {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cases/criterion_b.json");
    serde_json::from_slice(&std::fs::read(path).expect("read corpus")).expect("parse corpus")
}

fn metric(best: Option<f64>, lower: Option<f64>, upper: Option<f64>) -> Option<MetricInput> {
    best.map(|best| match (lower, upper) {
        (Some(lower), Some(upper)) => MetricInput::bounded(best, lower, upper),
        _ => MetricInput::point(best),
    })
}

#[test]
fn corpus_is_not_empty() {
    // Guards against a silently unreadable or renamed fixture file making the whole
    // conformance suite vacuously pass.
    assert!(corpus().cases.len() >= 28, "corpus shrank unexpectedly");
}

#[test]
fn every_case_matches() {
    let corpus = corpus();
    let mut failures = Vec::new();

    for case in &corpus.cases {
        let subs = SubconditionsInput {
            clauses: case.clauses.clone(),
            locations: case.locations,
            no_plausible_threats: case.no_plausible_threats,
            locations_insufficient_information: case.locations_insufficient_information,
        };

        let summary = match criterion_b_from_parts(
            metric(case.eoo_km2, case.eoo_lower_km2, case.eoo_upper_km2),
            metric(case.aoo_cells, None, None),
            subs,
        ) {
            Ok(summary) => summary,
            Err(e) => {
                failures.push(format!("{}: unexpected error: {e}", case.id));
                continue;
            }
        };

        let by_criterion: HashMap<&str, &str> = summary
            .criteria
            .iter()
            .map(|c| (c.criterion.as_str(), c.category.as_str()))
            .collect();

        for (key, expected) in &case.expect {
            let actual = if key == "overall" {
                summary.overall.as_str()
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

        // Some cases also pin the pre-gate threshold outcome, which is what makes the
        // audit trail meaningful.
        for (key, expected) in &case.expect_threshold {
            let actual = summary
                .criteria
                .iter()
                .find(|c| c.criterion.eq_ignore_ascii_case(key))
                .and_then(|c| c.threshold_category.as_deref())
                .unwrap_or("<missing>");
            if actual != expected {
                failures.push(format!(
                    "{}: {key} threshold expected {expected:?}, got {actual:?}",
                    case.id
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} conformance cases failed:\n  - {}",
        failures.len(),
        corpus.cases.len(),
        failures.join("\n  - ")
    );
}
