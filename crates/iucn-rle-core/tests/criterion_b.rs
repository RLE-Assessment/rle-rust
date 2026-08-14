//! Criterion B: restricted geographic distribution.
//!
//! The point of this module is the sub-condition gate. rle-python computes the
//! spatial thresholds and then says, in a docstring, that a real listing also
//! requires sub-condition (a), (b), or (c). That caveat is invisible to code and
//! easy to forget in a report. Here it is data, and it changes the answer.

use iucn_rle_core::{
    criterion_b, Basis, Category, ConditionStatus, CriterionId, Estimate, Note, Subcondition,
    SubconditionAssessment, ThresholdTable,
};

fn table() -> &'static ThresholdTable {
    ThresholdTable::v2_2024()
}

fn eoo(km2: f64) -> Estimate {
    Estimate::point(km2, Basis::Estimated)
}

fn aoo(cells: f64) -> Estimate {
    Estimate::point(cells, Basis::Estimated)
}

fn met(subcondition: Subcondition) -> SubconditionAssessment {
    SubconditionAssessment::new(subcondition, ConditionStatus::Met)
}

fn not_met(subcondition: Subcondition) -> SubconditionAssessment {
    SubconditionAssessment::new(subcondition, ConditionStatus::NotMet)
}

const ALL_SUBCONDITIONS: [Subcondition; 3] = [
    Subcondition::ContinuingDecline,
    Subcondition::ThreateningProcesses,
    Subcondition::FewLocations,
];

#[test]
fn a_met_subcondition_yields_a_definite_listing() {
    let subs = [met(Subcondition::ContinuingDecline)];
    let assessment = criterion_b(Some(eoo(15_000.0)), Some(aoo(15.0)), &subs, table()).unwrap();

    assert_eq!(assessment.category().best(), Category::En);
    assert!(assessment.category().is_point());
    assert_eq!(assessment.category().to_string(), "EN");
}

#[test]
fn unassessed_subconditions_yield_an_honest_range_not_a_bare_category() {
    // The metrics say EN. Nobody has looked at (a), (b), or (c), so the true
    // answer is somewhere between LC and EN. Reporting a bare "EN" would
    // overclaim; reporting LC would understate. Report the range.
    let assessment = criterion_b(Some(eoo(15_000.0)), Some(aoo(15.0)), &[], table()).unwrap();

    let category = assessment.category();
    assert_eq!(category.best(), Category::En, "best stays precautionary");
    assert_eq!(category.plausible_most(), Category::En);
    assert_eq!(category.plausible_least(), Category::Lc);
    assert_eq!(category.to_string(), "EN (LC-EN)");
}

#[test]
fn unassessed_subconditions_are_reported_as_a_note() {
    let assessment = criterion_b(Some(eoo(15_000.0)), Some(aoo(15.0)), &[], table()).unwrap();

    let pending = assessment.notes().iter().find_map(|note| match note {
        Note::SubconditionsNotAssessed { pending } => Some(pending),
        _ => None,
    });

    assert_eq!(
        pending.expect("a note naming the unassessed sub-conditions"),
        &ALL_SUBCONDITIONS.to_vec()
    );
}

#[test]
fn all_subconditions_ruled_out_means_the_ecosystem_does_not_qualify() {
    // Metrics are in the EN range, but every sub-condition has been assessed and
    // none is met, so criterion B is not triggered at all.
    let subs: Vec<_> = ALL_SUBCONDITIONS.into_iter().map(not_met).collect();
    let assessment = criterion_b(Some(eoo(15_000.0)), Some(aoo(15.0)), &subs, table()).unwrap();

    assert_eq!(assessment.category().best(), Category::Lc);
    assert!(assessment.category().is_point());
}

#[test]
fn the_overall_category_is_the_most_threatened_sub_criterion() {
    // EOO 15 000 km2 is EN; AOO 1 cell is CR. CR governs.
    let subs = [met(Subcondition::ThreateningProcesses)];
    let assessment = criterion_b(Some(eoo(15_000.0)), Some(aoo(1.0)), &subs, table()).unwrap();

    assert_eq!(
        assessment
            .result(CriterionId::B1)
            .unwrap()
            .category()
            .best(),
        Category::En
    );
    assert_eq!(
        assessment
            .result(CriterionId::B2)
            .unwrap()
            .category()
            .best(),
        Category::Cr
    );
    assert_eq!(assessment.category().best(), Category::Cr);
}

#[test]
fn a_missing_metric_leaves_that_sub_criterion_unevaluated() {
    let subs = [met(Subcondition::ContinuingDecline)];
    let assessment = criterion_b(None, Some(aoo(15.0)), &subs, table()).unwrap();

    let b1 = assessment.result(CriterionId::B1).unwrap();
    assert_eq!(b1.category().best(), Category::Ne);
    assert!(b1.metric().is_none());

    // A sub-criterion nobody could evaluate must not drag the overall result
    // down to NE — the evaluated one governs.
    assert_eq!(assessment.category().best(), Category::En);
}

#[test]
fn no_metrics_at_all_means_not_evaluated() {
    let assessment = criterion_b(None, None, &[], table()).unwrap();
    assert_eq!(assessment.category().best(), Category::Ne);
}

#[test]
fn the_threshold_outcome_is_retained_separately_from_the_gated_one() {
    // Auditability: a reviewer must be able to see what the spatial thresholds
    // said before the sub-condition gate was applied.
    let assessment = criterion_b(Some(eoo(15_000.0)), Some(aoo(15.0)), &[], table()).unwrap();
    let b1 = assessment.result(CriterionId::B1).unwrap();

    assert_eq!(b1.threshold_category().unwrap().best(), Category::En);
    assert_eq!(b1.category().to_string(), "EN (LC-EN)");
}

#[test]
fn provenance_pins_the_guidelines_edition_and_threshold_digest() {
    let assessment = criterion_b(Some(eoo(15_000.0)), None, &[], table()).unwrap();
    let provenance = assessment.provenance();

    assert_eq!(provenance.guidelines_version(), "2.0");
    assert_eq!(provenance.thresholds_sha256().len(), 64);
    assert!(provenance.engine().starts_with("iucn-rle-core"));
}

#[test]
fn metric_uncertainty_and_subcondition_uncertainty_compose() {
    // EOO plausibly 15 000-25 000 (EN to VU), and sub-conditions unassessed
    // (so LC remains possible). The reported range must span all of it.
    let e = Estimate::bounded(20_000.0, 15_000.0, 25_000.0, Basis::Inferred);
    let assessment = criterion_b(Some(e), None, &[], table()).unwrap();

    let category = assessment.category();
    assert_eq!(category.best(), Category::En);
    assert_eq!(category.plausible_most(), Category::En);
    assert_eq!(category.plausible_least(), Category::Lc);
}
