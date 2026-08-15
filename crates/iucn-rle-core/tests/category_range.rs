//! Plausible bounds on a category.
//!
//! Nothing in the existing ecosystem can express "EN (VU-CR)" — neither
//! rle-python nor redlistr represents uncertainty in the outcome. This is that
//! type, and its display format is what assessments actually print.

use iucn_rle_core::{Category, CategoryRange};

#[test]
fn a_point_range_displays_as_a_bare_code() {
    let range = CategoryRange::point(Category::En);
    assert_eq!(range.to_string(), "EN");
    assert!(range.is_point());
}

#[test]
fn a_span_displays_best_then_least_to_most_threatened() {
    // Best estimate EN, plausibly anywhere from VU to CR.
    let range = CategoryRange::span(Category::En, Category::Vu, Category::Cr);
    assert_eq!(range.to_string(), "EN (VU-CR)");
    assert!(!range.is_point());
}

#[test]
fn span_normalises_bounds_given_in_either_order() {
    // Callers should not have to know which of two categories is worse.
    let a = CategoryRange::span(Category::En, Category::Cr, Category::Vu);
    let b = CategoryRange::span(Category::En, Category::Vu, Category::Cr);
    assert_eq!(a, b);
    assert_eq!(a.to_string(), "EN (VU-CR)");
}

#[test]
fn span_records_which_bound_is_most_threatened() {
    let range = CategoryRange::span(Category::En, Category::Vu, Category::Cr);
    assert_eq!(range.plausible_most(), Category::Cr);
    assert_eq!(range.plausible_least(), Category::Vu);
    assert_eq!(range.best(), Category::En);
}

#[test]
fn combining_criteria_takes_the_most_threatened_of_each_bound() {
    // Overall assessment = most threatened criterion, applied pointwise so the
    // uncertainty of the governing criterion survives into the headline result.
    let b1 = CategoryRange::span(Category::Vu, Category::Lc, Category::Vu);
    let b2 = CategoryRange::span(Category::En, Category::Vu, Category::Cr);

    let overall = CategoryRange::most_threatened([b1, b2]).unwrap();

    assert_eq!(overall.best(), Category::En);
    assert_eq!(overall.plausible_least(), Category::Vu);
    assert_eq!(overall.plausible_most(), Category::Cr);
    assert_eq!(overall.to_string(), "EN (VU-CR)");
}

#[test]
fn combining_nothing_yields_nothing() {
    assert_eq!(CategoryRange::most_threatened([]), None);
}
