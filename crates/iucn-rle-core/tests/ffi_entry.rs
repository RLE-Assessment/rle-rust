//! The single entry point every language binding calls.

use iucn_rle_core::ffi::{criterion_b_from_parts, MetricInput, SubconditionInput};

fn sub(letter: &str, status: &str) -> SubconditionInput {
    SubconditionInput {
        sub: letter.to_owned(),
        status: status.to_owned(),
    }
}

#[test]
fn point_metrics_produce_a_summary() {
    let summary =
        criterion_b_from_parts(Some(MetricInput::point(15_000.0)), None, &[sub("a", "met")])
            .unwrap();

    assert_eq!(summary.overall, "EN");
}

#[test]
fn bounded_metrics_carry_uncertainty_through() {
    let summary = criterion_b_from_parts(
        Some(MetricInput::bounded(20_000.0, 15_000.0, 25_000.0)),
        None,
        &[sub("a", "met")],
    )
    .unwrap();

    assert_eq!(summary.overall, "EN (VU-EN)");
}

#[test]
fn omitting_subconditions_yields_the_provisional_range() {
    let summary = criterion_b_from_parts(Some(MetricInput::point(15_000.0)), None, &[]).unwrap();

    assert_eq!(summary.overall, "EN (LC-EN)");
}

#[test]
fn an_unknown_subcondition_letter_is_a_clear_error() {
    let err = criterion_b_from_parts(Some(MetricInput::point(15_000.0)), None, &[sub("z", "met")])
        .unwrap_err();

    // Bindings surface this string directly to users of four languages, so it
    // must name the offending value and the accepted ones.
    assert!(err.contains('z'), "error should name the bad value: {err}");
    assert!(
        err.contains("a|b|c"),
        "error should list valid values: {err}"
    );
}

#[test]
fn an_unknown_status_is_a_clear_error() {
    let err = criterion_b_from_parts(
        Some(MetricInput::point(15_000.0)),
        None,
        &[sub("a", "maybe")],
    )
    .unwrap_err();

    assert!(
        err.contains("maybe"),
        "error should name the bad value: {err}"
    );
    assert!(
        err.contains("not_assessed"),
        "error should list valid values: {err}"
    );
}

#[test]
fn both_metrics_absent_is_not_an_error_but_not_evaluated() {
    let summary = criterion_b_from_parts(None, None, &[]).unwrap();
    assert_eq!(summary.overall, "NE");
}
