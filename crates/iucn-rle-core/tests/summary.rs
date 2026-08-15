//! The flat, string-based view every language binding returns.
//!
//! Rich Rust types do not cross an FFI boundary well, and five bindings should
//! not each invent their own flattening. `Summary` is that shape, defined once:
//! plain strings and booleans, serde-serialisable, with categories rendered as
//! their display form so a caller compares `"EN (LC-EN)"` rather than
//! reconstructing it from three enum fields.

use iucn_rle_core::{criterion_b, Basis, Estimate, Subconditions, ThresholdTable};

fn assessment() -> iucn_rle_core::Assessment {
    criterion_b(
        Some(Estimate::point(15_000.0, Basis::Estimated)),
        Some(Estimate::point(15.0, Basis::Estimated)),
        &Subconditions::new(),
        ThresholdTable::v2_2024(),
    )
    .unwrap()
}

#[test]
fn summary_renders_categories_as_display_strings() {
    let summary = assessment().summary();

    assert_eq!(summary.overall, "EN (LC-EN)");
    assert_eq!(summary.overall_best, "EN");
    assert!(summary.is_threatened);
}

#[test]
fn summary_lists_each_sub_criterion() {
    let summary = assessment().summary();

    let b1 = summary
        .criteria
        .iter()
        .find(|c| c.criterion == "B1")
        .expect("B1 present");
    assert_eq!(b1.category, "EN (LC-EN)");
    assert_eq!(b1.threshold_category.as_deref(), Some("EN"));

    // B1, B2 and B3. B3 is always reported, even when unevaluated, so a reader can see
    // it was considered rather than silently omitted.
    assert_eq!(summary.criteria.len(), 3);
    assert!(summary.criteria.iter().any(|c| c.criterion == "B3"));
}

#[test]
fn summary_carries_provenance() {
    let summary = assessment().summary();

    assert_eq!(summary.guidelines_version, "2.0");
    assert_eq!(summary.thresholds_sha256.len(), 64);
    assert!(summary.engine.starts_with("iucn-rle-core"));
}

#[test]
fn notes_render_as_human_readable_strings() {
    let summary = assessment().summary();

    assert!(
        summary
            .notes
            .iter()
            .any(|n| n.contains("sub-condition") && n.contains("not assessed")),
        "expected a note about unassessed sub-conditions, got {:?}",
        summary.notes
    );
}

#[test]
fn summary_round_trips_through_json() {
    // Every binding either serialises this or reads its fields directly, so a
    // broken round-trip breaks all five at once.
    let summary = assessment().summary();
    let json = serde_json::to_string(&summary).unwrap();
    let back: iucn_rle_core::Summary = serde_json::from_str(&json).unwrap();

    assert_eq!(summary, back);
}
