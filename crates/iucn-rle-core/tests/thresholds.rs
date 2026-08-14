//! Criterion B threshold semantics.
//!
//! Transcribed from IUCN RLE Guidelines v2.0 (2024) Section 6.2, p.66, and
//! matching rle-python's `_category_for` (rle.py:103-114) exactly:
//! inclusive upper bounds, first match wins, above every bound yields LC, and
//! NT is never emitted because it has no numeric breakpoint.

use iucn_rle_core::{Category, CriterionId, ThresholdTable};

fn table() -> &'static ThresholdTable {
    ThresholdTable::v2_2024()
}

#[test]
fn b1_boundaries_are_inclusive_upper_bounds() {
    let t = table();
    // EOO in km². Each bound belongs to the more threatened category.
    assert_eq!(t.classify(CriterionId::B1, 2_000.0).unwrap(), Category::Cr);
    assert_eq!(t.classify(CriterionId::B1, 2_000.01).unwrap(), Category::En);
    assert_eq!(t.classify(CriterionId::B1, 20_000.0).unwrap(), Category::En);
    assert_eq!(t.classify(CriterionId::B1, 20_001.0).unwrap(), Category::Vu);
    assert_eq!(t.classify(CriterionId::B1, 50_000.0).unwrap(), Category::Vu);
}

#[test]
fn b2_boundaries_are_inclusive_upper_bounds() {
    let t = table();
    // AOO as a count of occupied 10x10 km cells.
    assert_eq!(t.classify(CriterionId::B2, 2.0).unwrap(), Category::Cr);
    assert_eq!(t.classify(CriterionId::B2, 3.0).unwrap(), Category::En);
    assert_eq!(t.classify(CriterionId::B2, 20.0).unwrap(), Category::En);
    assert_eq!(t.classify(CriterionId::B2, 21.0).unwrap(), Category::Vu);
    assert_eq!(t.classify(CriterionId::B2, 50.0).unwrap(), Category::Vu);
}

#[test]
fn a_metric_above_every_bound_is_least_concern() {
    let t = table();
    assert_eq!(t.classify(CriterionId::B1, 50_001.0).unwrap(), Category::Lc);
    assert_eq!(t.classify(CriterionId::B2, 51.0).unwrap(), Category::Lc);
}

#[test]
fn near_threatened_is_never_emitted_by_a_threshold() {
    // NT is a judgement call with no numeric breakpoint. If a threshold table
    // ever yields it, the table is wrong.
    let t = table();
    for step in 0..2_000 {
        let eoo = f64::from(step) * 100.0;
        assert_ne!(t.classify(CriterionId::B1, eoo).unwrap(), Category::Nt);
    }
}

#[test]
fn classifying_is_monotone_in_risk() {
    // A larger EOO can never be more threatened than a smaller one.
    let t = table();
    let mut previous = Category::Co;
    for step in 0..2_000 {
        let eoo = f64::from(step) * 100.0;
        let category = t.classify(CriterionId::B1, eoo).unwrap();
        assert!(
            category >= previous,
            "risk increased as EOO grew: {eoo} km2 gave {category}"
        );
        previous = category;
    }
}

#[test]
fn a_criterion_with_no_table_is_an_error() {
    // Criterion E is a bespoke simulation; there is no threshold table for it.
    assert!(table().classify(CriterionId::E, 0.5).is_err());
}

#[test]
fn the_table_records_which_guidelines_it_encodes() {
    let t = table();
    assert_eq!(t.guidelines_version(), "2.0");
    assert_eq!(t.guidelines_year(), 2024);
    // Pinned into every assessment's provenance so a reviewer can prove which
    // edition produced a category.
    assert_eq!(t.sha256().len(), 64);
}
