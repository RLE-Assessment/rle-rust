//! The single entry point every language binding calls.

use iucn_rle_core::ffi::{
    criterion_b_from_parts, MetricInput, SubconditionInput, SubconditionsInput,
};

fn clause(sub: &str, status: &str) -> SubconditionInput {
    SubconditionInput {
        sub: sub.to_owned(),
        status: status.to_owned(),
    }
}

fn with(clauses: Vec<SubconditionInput>) -> SubconditionsInput {
    SubconditionsInput {
        clauses,
        ..Default::default()
    }
}

#[test]
fn point_metrics_produce_a_summary() {
    let summary = criterion_b_from_parts(
        Some(MetricInput::point(15_000.0)),
        None,
        with(vec![clause("a", "met")]),
    )
    .unwrap();

    assert_eq!(summary.overall, "EN");
}

#[test]
fn bounded_metrics_carry_uncertainty_through() {
    let summary = criterion_b_from_parts(
        Some(MetricInput::bounded(20_000.0, 15_000.0, 25_000.0)),
        None,
        with(vec![clause("a", "met")]),
    )
    .unwrap();

    assert_eq!(summary.overall, "EN (VU-EN)");
}

#[test]
fn omitting_everything_yields_the_provisional_range() {
    let summary = criterion_b_from_parts(
        Some(MetricInput::point(15_000.0)),
        None,
        SubconditionsInput::default(),
    )
    .unwrap();

    assert_eq!(summary.overall, "EN (LC-EN)");
}

#[test]
fn a_bare_clause_a_sets_every_decline_aspect() {
    // Callers that do not distinguish a(i)/a(ii)/a(iii) should not have to.
    let all = criterion_b_from_parts(
        Some(MetricInput::point(15_000.0)),
        None,
        with(vec![clause("a", "met")]),
    )
    .unwrap();
    let one = criterion_b_from_parts(
        Some(MetricInput::point(15_000.0)),
        None,
        with(vec![clause("a.ii", "met")]),
    )
    .unwrap();

    assert_eq!(all.overall, one.overall);
}

#[test]
fn decline_aspects_are_addressable_individually() {
    for aspect in ["a.i", "a.ii", "a.iii", "i", "spatial_extent"] {
        let summary = criterion_b_from_parts(
            Some(MetricInput::point(15_000.0)),
            None,
            with(vec![clause(aspect, "met")]),
        )
        .unwrap_or_else(|e| panic!("{aspect} should be accepted: {e}"));
        assert_eq!(summary.overall, "EN", "aspect {aspect}");
    }
}

#[test]
fn clause_c_as_a_status_is_rejected_with_a_pointer_to_the_right_field() {
    // (c) is a count, not a status. Silently accepting a boolean here is exactly the
    // bug this refactor fixed, so the error has to be actionable.
    let err = criterion_b_from_parts(
        Some(MetricInput::point(15_000.0)),
        None,
        with(vec![clause("c", "met")]),
    )
    .unwrap_err();

    assert!(
        err.contains("locations"),
        "error should name the fix: {err}"
    );
}

#[test]
fn an_unknown_clause_is_a_clear_error() {
    let err = criterion_b_from_parts(
        Some(MetricInput::point(15_000.0)),
        None,
        with(vec![clause("z", "met")]),
    )
    .unwrap_err();

    assert!(err.contains('z'), "error should name the bad value: {err}");
    assert!(err.contains("a|b"), "error should list valid values: {err}");
}

#[test]
fn an_unknown_status_is_a_clear_error() {
    let err = criterion_b_from_parts(
        Some(MetricInput::point(15_000.0)),
        None,
        with(vec![clause("a", "maybe")]),
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
fn over_specified_locations_are_rejected() {
    // A count and "no plausible threats" are contradictory claims; guessing which the
    // caller meant would silently change a category.
    let err = criterion_b_from_parts(
        Some(MetricInput::point(15_000.0)),
        None,
        SubconditionsInput {
            locations: Some(3),
            no_plausible_threats: true,
            ..Default::default()
        },
    )
    .unwrap_err();

    assert!(err.contains("over-specified"), "{err}");
}

#[test]
fn both_metrics_absent_is_not_an_error_but_not_evaluated() {
    let summary = criterion_b_from_parts(None, None, SubconditionsInput::default()).unwrap();
    assert_eq!(summary.overall, "NE");
}
