//! Metric estimates and their mapping onto category ranges.

// These assertions check that a literal handed in comes back unchanged, so exact
// comparison is correct — no arithmetic happens in between to introduce error.
#![allow(clippy::float_cmp)]

use iucn_rle_core::{Basis, Category, CriterionId, Estimate, ThresholdTable};

#[test]
fn a_point_estimate_has_no_bounds() {
    let e = Estimate::point(15_000.0, Basis::Observed);
    assert_eq!(e.best(), 15_000.0);
    assert_eq!(e.lower(), None);
    assert_eq!(e.upper(), None);
    assert!(e.is_point());
    assert_eq!(e.basis(), Basis::Observed);
}

#[test]
fn a_bounded_estimate_reports_its_bounds() {
    let e = Estimate::bounded(20_000.0, 15_000.0, 25_000.0, Basis::Inferred);
    assert_eq!(e.best(), 20_000.0);
    assert_eq!(e.lower(), Some(15_000.0));
    assert_eq!(e.upper(), Some(25_000.0));
    assert!(!e.is_point());
}

#[test]
fn bounds_given_in_either_order_are_normalised() {
    let a = Estimate::bounded(20_000.0, 25_000.0, 15_000.0, Basis::Inferred);
    let b = Estimate::bounded(20_000.0, 15_000.0, 25_000.0, Basis::Inferred);
    assert_eq!(a, b);
}

#[test]
fn a_point_estimate_classifies_to_a_point_range() {
    let table = ThresholdTable::v2_2024();
    let e = Estimate::point(15_000.0, Basis::Observed);

    let range = table.classify_estimate(CriterionId::B1, &e).unwrap();

    assert!(range.is_point());
    assert_eq!(range.best(), Category::En);
    assert_eq!(range.to_string(), "EN");
}

#[test]
fn uncertainty_in_the_metric_becomes_uncertainty_in_the_category() {
    // EOO best 20 000 km2, plausibly 15 000 to 25 000. For B1 a *smaller* EOO is
    // worse, so the lower metric bound is the more threatened category.
    let table = ThresholdTable::v2_2024();
    let e = Estimate::bounded(20_000.0, 15_000.0, 25_000.0, Basis::Inferred);

    let range = table.classify_estimate(CriterionId::B1, &e).unwrap();

    assert_eq!(range.best(), Category::En);
    assert_eq!(range.plausible_most(), Category::En); // from 15 000
    assert_eq!(range.plausible_least(), Category::Vu); // from 25 000
    assert_eq!(range.to_string(), "EN (VU-EN)");
}

#[test]
fn bounds_that_straddle_two_breakpoints_span_three_categories() {
    let table = ThresholdTable::v2_2024();
    let e = Estimate::bounded(20_000.0, 1_500.0, 60_000.0, Basis::Suspected);

    let range = table.classify_estimate(CriterionId::B1, &e).unwrap();

    assert_eq!(range.best(), Category::En);
    assert_eq!(range.plausible_most(), Category::Cr); // from 1 500
    assert_eq!(range.plausible_least(), Category::Lc); // from 60 000
    assert_eq!(range.to_string(), "EN (LC-CR)");
}
