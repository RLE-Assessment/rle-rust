//! The Rust runner for the cross-language conformance corpus.
//!
//! Python, R, JavaScript, and (later) Julia run the identical JSON file. A
//! binding is not finished until it passes this corpus, which is what makes
//! "all surfaces agree" a checkable claim rather than an aspiration.

use std::path::PathBuf;

use iucn_rle_core::{
    criterion_b, Basis, ConditionStatus, CriterionId, Estimate, Subcondition,
    SubconditionAssessment, ThresholdTable,
};
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
    subconditions: Vec<Sub>,
    expect: Expect,
}

#[derive(Deserialize)]
struct Sub {
    sub: String,
    status: String,
}

#[derive(Deserialize)]
struct Expect {
    b1: String,
    b2: String,
    overall: String,
}

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cases/criterion_b.json")
}

fn parse_subcondition(letter: &str) -> Subcondition {
    match letter {
        "a" => Subcondition::ContinuingDecline,
        "b" => Subcondition::ThreateningProcesses,
        "c" => Subcondition::FewLocations,
        other => panic!("unknown sub-condition letter {other:?}"),
    }
}

fn parse_status(status: &str) -> ConditionStatus {
    match status {
        "met" => ConditionStatus::Met,
        "not_met" => ConditionStatus::NotMet,
        "not_assessed" => ConditionStatus::NotAssessed,
        other => panic!("unknown status {other:?}"),
    }
}

#[test]
fn corpus_is_not_empty() {
    // Guards against a silently unreadable or renamed fixture file making the
    // whole conformance suite vacuously pass.
    let corpus: Corpus =
        serde_json::from_slice(&std::fs::read(corpus_path()).expect("read corpus"))
            .expect("parse corpus");
    assert!(corpus.cases.len() >= 20, "corpus shrank unexpectedly");
}

#[test]
fn every_case_matches() {
    let corpus: Corpus =
        serde_json::from_slice(&std::fs::read(corpus_path()).expect("read corpus"))
            .expect("parse corpus");

    let mut failures = Vec::new();

    for case in &corpus.cases {
        let eoo = case
            .eoo_km2
            .map(|best| match (case.eoo_lower_km2, case.eoo_upper_km2) {
                (Some(lo), Some(hi)) => Estimate::bounded(best, lo, hi, Basis::Estimated),
                _ => Estimate::point(best, Basis::Estimated),
            });
        let aoo = case
            .aoo_cells
            .map(|cells| Estimate::point(cells, Basis::Estimated));

        let subs: Vec<SubconditionAssessment> = case
            .subconditions
            .iter()
            .map(|s| {
                SubconditionAssessment::new(parse_subcondition(&s.sub), parse_status(&s.status))
            })
            .collect();

        let assessment =
            criterion_b(eoo, aoo, &subs, ThresholdTable::v2_2024()).expect("criterion B");

        let actual = (
            assessment
                .result(CriterionId::B1)
                .unwrap()
                .category()
                .to_string(),
            assessment
                .result(CriterionId::B2)
                .unwrap()
                .category()
                .to_string(),
            assessment.category().to_string(),
        );
        let expected = (
            case.expect.b1.clone(),
            case.expect.b2.clone(),
            case.expect.overall.clone(),
        );

        if actual != expected {
            failures.push(format!(
                "{}\n     expected b1={} b2={} overall={}\n     actual   b1={} b2={} overall={}",
                case.id, expected.0, expected.1, expected.2, actual.0, actual.1, actual.2
            ));
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
