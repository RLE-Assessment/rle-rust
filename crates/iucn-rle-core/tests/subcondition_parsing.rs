//! Parsing sub-conditions and statuses from strings.
//!
//! Every language binding receives these as strings across the FFI boundary, so
//! the parsing lives here once rather than being reimplemented five times.

use iucn_rle_core::{ConditionStatus, Subcondition};

#[test]
fn subconditions_parse_from_guidelines_letters() {
    assert_eq!(
        "a".parse::<Subcondition>().unwrap(),
        Subcondition::ContinuingDecline
    );
    assert_eq!(
        "b".parse::<Subcondition>().unwrap(),
        Subcondition::ThreateningProcesses
    );
    assert_eq!(
        "c".parse::<Subcondition>().unwrap(),
        Subcondition::FewLocations
    );
}

#[test]
fn subconditions_parse_from_snake_case_names() {
    assert_eq!(
        "continuing_decline".parse::<Subcondition>().unwrap(),
        Subcondition::ContinuingDecline
    );
    assert_eq!(
        "few_locations".parse::<Subcondition>().unwrap(),
        Subcondition::FewLocations
    );
}

#[test]
fn an_unknown_subcondition_fails() {
    assert!("d".parse::<Subcondition>().is_err());
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
    // "unknown" is a plausible synonym a caller might reach for, but accepting
    // it silently would blur the not-met / not-assessed distinction the whole
    // gate depends on. Reject it and make the caller choose.
    assert!("unknown".parse::<ConditionStatus>().is_err());
}
