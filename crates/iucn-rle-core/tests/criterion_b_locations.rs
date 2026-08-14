//! Sub-condition (c) is a category-dependent threshold, not a boolean.
//!
//! IUCN (2024) Guidelines v2.0, Appendix 1 (Criteria v2.1), pp. 154-155. Clause (c)
//! reads "Ecosystem exists at 1 threat-defined location" for CR, "<= 5" for EN, and
//! "<= 10" for VU. A sub-criterion is therefore met at a given category only when the
//! spatial threshold AND the gate both hold *at that same category*, which means the
//! evaluation has to run per level rather than picking a threshold category first and
//! then applying a boolean gate.

use iucn_rle_core::{
    criterion_b, Basis, Category, ConditionStatus, CriterionId, DeclineAspect, Estimate,
    Subconditions, ThreatLocations, ThresholdTable,
};

fn table() -> &'static ThresholdTable {
    ThresholdTable::v2_2024()
}

fn eoo(km2: f64) -> Estimate {
    Estimate::point(km2, Basis::Estimated)
}

/// Everything assessed and absent except the location count, so (c) alone decides.
fn only_locations(count: u32) -> Subconditions {
    Subconditions::new()
        .with_decline(DeclineAspect::SpatialExtent, ConditionStatus::NotMet)
        .with_decline(DeclineAspect::EnvironmentalQuality, ConditionStatus::NotMet)
        .with_decline(DeclineAspect::BioticInteractions, ConditionStatus::NotMet)
        .with_threatening_processes(ConditionStatus::NotMet)
        .with_locations(ThreatLocations::Count(count))
}

#[test]
fn regression_three_locations_in_the_cr_band_is_en_not_cr() {
    // THE BUG. EOO 1,500 km2 sits in the CR band, but clause (c) at CR demands exactly
    // one threat-defined location. With three, only the EN clause (<= 5) is satisfied,
    // so the listing is EN. Treating (c) as a boolean gate on a pre-computed CR
    // threshold category returns CR, which overstates the threat by a full category.
    let assessment = criterion_b(Some(eoo(1_500.0)), None, &only_locations(3), table()).unwrap();

    assert_eq!(
        assessment
            .result(CriterionId::B1)
            .unwrap()
            .category()
            .best(),
        Category::En
    );
}

#[test]
fn one_location_in_the_cr_band_is_cr() {
    let assessment = criterion_b(Some(eoo(1_500.0)), None, &only_locations(1), table()).unwrap();
    assert_eq!(
        assessment
            .result(CriterionId::B1)
            .unwrap()
            .category()
            .best(),
        Category::Cr
    );
}

#[test]
fn eight_locations_in_the_cr_band_is_vu() {
    // Too many for CR (1) and for EN (<= 5), but within VU (<= 10).
    let assessment = criterion_b(Some(eoo(1_500.0)), None, &only_locations(8), table()).unwrap();
    assert_eq!(
        assessment
            .result(CriterionId::B1)
            .unwrap()
            .category()
            .best(),
        Category::Vu
    );
}

#[test]
fn eleven_locations_fails_clause_c_entirely() {
    // Above every clause (c) bound, and (a) and (b) are ruled out, so criterion B is
    // not triggered no matter how small the EOO is.
    let assessment = criterion_b(Some(eoo(1_500.0)), None, &only_locations(11), table()).unwrap();
    assert_eq!(
        assessment
            .result(CriterionId::B1)
            .unwrap()
            .category()
            .best(),
        Category::Lc
    );
}

#[test]
fn the_metric_still_caps_the_outcome() {
    // One location satisfies clause (c) at every level, but an EOO of 30,000 km2 only
    // reaches the VU band, so VU is the answer.
    let assessment = criterion_b(Some(eoo(30_000.0)), None, &only_locations(1), table()).unwrap();
    assert_eq!(
        assessment
            .result(CriterionId::B1)
            .unwrap()
            .category()
            .best(),
        Category::Vu
    );
}

#[test]
fn a_met_decline_aspect_satisfies_the_gate_at_every_level() {
    // Clause (a) is met if ANY of (i) spatial extent, (ii) environmental quality or
    // (iii) biotic interactions is declining, and unlike (c) it is not level-dependent.
    let subs = Subconditions::new()
        .with_decline(DeclineAspect::BioticInteractions, ConditionStatus::Met)
        .with_locations(ThreatLocations::NoPlausibleThreats);

    let assessment = criterion_b(Some(eoo(1_500.0)), None, &subs, table()).unwrap();
    assert_eq!(
        assessment
            .result(CriterionId::B1)
            .unwrap()
            .category()
            .best(),
        Category::Cr
    );
}

#[test]
fn no_plausible_threats_means_clause_c_is_not_met() {
    // Box 13 step 5: "Where there are no plausible threats to the ecosystem type,
    // subcriteria B1(c), B2(c) and B3 are not met." That is a finding, not ignorance.
    let subs = Subconditions::new()
        .with_decline(DeclineAspect::SpatialExtent, ConditionStatus::NotMet)
        .with_decline(DeclineAspect::EnvironmentalQuality, ConditionStatus::NotMet)
        .with_decline(DeclineAspect::BioticInteractions, ConditionStatus::NotMet)
        .with_threatening_processes(ConditionStatus::NotMet)
        .with_locations(ThreatLocations::NoPlausibleThreats);

    let assessment = criterion_b(Some(eoo(1_500.0)), None, &subs, table()).unwrap();
    let b1 = assessment.result(CriterionId::B1).unwrap();

    assert_eq!(b1.category().best(), Category::Lc);
    assert!(b1.category().is_point(), "a finding, so no uncertainty");
}

#[test]
fn insufficient_information_about_locations_is_data_deficient() {
    // Box 13 step 5 again: this "should be distinguished from cases in which there is
    // insufficient information to assess the number of threat-defined locations
    // (i.e. a Data Deficient outcome)."
    let subs = Subconditions::new()
        .with_decline(DeclineAspect::SpatialExtent, ConditionStatus::NotMet)
        .with_decline(DeclineAspect::EnvironmentalQuality, ConditionStatus::NotMet)
        .with_decline(DeclineAspect::BioticInteractions, ConditionStatus::NotMet)
        .with_threatening_processes(ConditionStatus::NotMet)
        .with_locations(ThreatLocations::InsufficientInformation);

    let assessment = criterion_b(Some(eoo(1_500.0)), None, &subs, table()).unwrap();
    assert_eq!(
        assessment
            .result(CriterionId::B1)
            .unwrap()
            .category()
            .best(),
        Category::Dd
    );
}

#[test]
fn an_unassessed_gate_still_produces_a_range() {
    // Nothing supplied at all: the metric says CR, but no clause is established, so the
    // outcome spans LC to CR. Guidelines 6.3.2 p.70 asks for exactly this propagation.
    let assessment = criterion_b(Some(eoo(1_500.0)), None, &Subconditions::new(), table()).unwrap();
    let category = assessment.result(CriterionId::B1).unwrap().category();

    assert_eq!(category.best(), Category::Cr);
    assert_eq!(category.plausible_most(), Category::Cr);
    assert_eq!(category.plausible_least(), Category::Lc);
}

#[test]
fn a_confirmed_lower_level_narrows_the_range() {
    // (c) confirms EN at three locations, but (a) is unassessed and could still lift
    // this to CR. The honest answer is bounded below by EN, not by LC.
    let subs = Subconditions::new().with_locations(ThreatLocations::Count(3));
    let assessment = criterion_b(Some(eoo(1_500.0)), None, &subs, table()).unwrap();
    let category = assessment.result(CriterionId::B1).unwrap().category();

    assert_eq!(category.plausible_most(), Category::Cr);
    assert_eq!(category.plausible_least(), Category::En);
    assert_eq!(category.to_string(), "CR (EN-CR)");
}
