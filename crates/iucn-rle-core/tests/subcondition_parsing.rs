//! Parsing sub-condition aspects and statuses from strings.
//!
//! Every language binding receives these as strings across the FFI boundary, so the
//! parsing lives here once rather than being reimplemented five times.

use iucn_rle_core::{ConditionStatus, DeclineAspect};

#[test]
fn decline_aspects_parse_from_roman_numerals() {
    // Clause (a) has three sub-parts in the criteria, addressed as i, ii and iii.
    assert_eq!(
        "i".parse::<DeclineAspect>().unwrap(),
        DeclineAspect::SpatialExtent
    );
    assert_eq!(
        "ii".parse::<DeclineAspect>().unwrap(),
        DeclineAspect::EnvironmentalQuality
    );
    assert_eq!(
        "iii".parse::<DeclineAspect>().unwrap(),
        DeclineAspect::BioticInteractions
    );
}

#[test]
fn decline_aspects_parse_from_snake_case_names() {
    assert_eq!(
        "spatial_extent".parse::<DeclineAspect>().unwrap(),
        DeclineAspect::SpatialExtent
    );
    assert_eq!(
        "biotic_interactions".parse::<DeclineAspect>().unwrap(),
        DeclineAspect::BioticInteractions
    );
}

#[test]
fn decline_aspects_expose_their_numerals() {
    assert_eq!(DeclineAspect::SpatialExtent.numeral(), "i");
    assert_eq!(DeclineAspect::EnvironmentalQuality.numeral(), "ii");
    assert_eq!(DeclineAspect::BioticInteractions.numeral(), "iii");
}

#[test]
fn an_unknown_aspect_fails() {
    assert!("iv".parse::<DeclineAspect>().is_err());
}

#[test]
fn statuses_parse_from_snake_case() {
    assert_eq!(
        "met".parse::<ConditionStatus>().unwrap(),
        ConditionStatus::Met
    );
    assert_eq!(
        "not_met".parse::<ConditionStatus>().unwrap(),
        ConditionStatus::NotMet
    );
    assert_eq!(
        "not_assessed".parse::<ConditionStatus>().unwrap(),
        ConditionStatus::NotAssessed
    );
}

#[test]
fn an_unknown_status_fails() {
    // "unknown" is a plausible synonym a caller might reach for, but accepting it
    // silently would blur the not-met / not-assessed distinction the whole gate
    // depends on. Reject it and make the caller choose.
    assert!("unknown".parse::<ConditionStatus>().is_err());
}
