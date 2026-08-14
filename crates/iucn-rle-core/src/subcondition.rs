//! Criterion B sub-conditions, modelled as data rather than prose.
//!
//! A listing under B1 or B2 requires the spatial threshold to be met **and** at
//! least one of sub-conditions (a), (b), or (c). `rle-python` states this in a
//! docstring, which means no caller can act on it and no report can be checked
//! against it. Here it is typed, so the engine can distinguish "we know a
//! sub-condition is met" from "nobody has looked" — and report the difference.

use core::str::FromStr;

use serde::{Deserialize, Serialize};

/// Returned when a string names no known sub-condition or status.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error("unrecognised {kind}: {value:?}; expected one of {expected}")]
pub struct ParseSubconditionError {
    /// What was being parsed, for the error message.
    pub kind: &'static str,
    /// The offending input.
    pub value: String,
    /// The accepted values.
    pub expected: &'static str,
}

/// A Criterion B sub-condition.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subcondition {
    /// (a) An observed or inferred continuing decline.
    ContinuingDecline,
    /// (b) Threatening processes likely to cause continuing decline within 20 years.
    ThreateningProcesses,
    /// (c) The ecosystem exists at very few threat-defined locations.
    FewLocations,
}

impl Subcondition {
    /// All three sub-conditions, in Guidelines order.
    pub const ALL: [Self; 3] = [
        Self::ContinuingDecline,
        Self::ThreateningProcesses,
        Self::FewLocations,
    ];

    /// The letter used in assessments: `'a'`, `'b'`, or `'c'`.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            Self::ContinuingDecline => 'a',
            Self::ThreateningProcesses => 'b',
            Self::FewLocations => 'c',
        }
    }
}

impl FromStr for Subcondition {
    type Err = ParseSubconditionError;

    /// Accepts the Guidelines letter (`"a"`) or the `snake_case` name
    /// (`"continuing_decline"`), case-insensitively.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalised = s.trim().to_ascii_lowercase();
        match normalised.as_str() {
            "a" | "continuing_decline" => Ok(Self::ContinuingDecline),
            "b" | "threatening_processes" => Ok(Self::ThreateningProcesses),
            "c" | "few_locations" => Ok(Self::FewLocations),
            _ => Err(ParseSubconditionError {
                kind: "sub-condition",
                value: s.to_owned(),
                expected: "a|b|c, or continuing_decline|threatening_processes|few_locations",
            }),
        }
    }
}

/// Whether a sub-condition holds.
///
/// `NotAssessed` is deliberately distinct from `NotMet`. Conflating them is what
/// turns "we have not checked" into "we checked and it is fine", which is the
/// error this whole type exists to prevent.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionStatus {
    /// Assessed and found to hold.
    Met,
    /// Assessed and found not to hold.
    NotMet,
    /// Nobody has assessed this.
    NotAssessed,
}

impl FromStr for ConditionStatus {
    type Err = ParseSubconditionError;

    /// Accepts `"met"`, `"not_met"`, or `"not_assessed"`, case-insensitively.
    ///
    /// Deliberately does **not** accept `"unknown"`. It reads as a synonym for
    /// either `not_met` or `not_assessed` depending on who wrote it, and blurring
    /// those two is exactly the error this type exists to prevent.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalised = s.trim().to_ascii_lowercase();
        match normalised.as_str() {
            "met" => Ok(Self::Met),
            "not_met" => Ok(Self::NotMet),
            "not_assessed" => Ok(Self::NotAssessed),
            _ => Err(ParseSubconditionError {
                kind: "sub-condition status",
                value: s.to_owned(),
                expected: "met|not_met|not_assessed",
            }),
        }
    }
}

/// One sub-condition together with its status and supporting evidence.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SubconditionAssessment {
    subcondition: Subcondition,
    status: ConditionStatus,
    /// Free text is permitted here and nowhere else, and never affects a category.
    evidence: Option<String>,
}

impl SubconditionAssessment {
    /// Record a sub-condition's status.
    #[must_use]
    pub const fn new(subcondition: Subcondition, status: ConditionStatus) -> Self {
        Self {
            subcondition,
            status,
            evidence: None,
        }
    }

    /// Attach supporting evidence. Narrative only; it never changes the outcome.
    #[must_use]
    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence = Some(evidence.into());
        self
    }

    /// Which sub-condition this concerns.
    #[must_use]
    pub const fn subcondition(&self) -> Subcondition {
        self.subcondition
    }

    /// Whether it holds.
    #[must_use]
    pub const fn status(&self) -> ConditionStatus {
        self.status
    }

    /// The supporting evidence, if any was recorded.
    #[must_use]
    pub fn evidence(&self) -> Option<&str> {
        self.evidence.as_deref()
    }
}

/// The combined effect of a set of sub-condition assessments.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Gate {
    /// At least one sub-condition is met: the threshold category stands.
    Satisfied,
    /// All three were assessed and none is met: the criterion is not triggered.
    Refuted,
    /// None is met but some were never assessed: the outcome is uncertain.
    Unknown,
}

/// Evaluate the sub-condition gate, and report which are still unassessed.
pub(crate) fn evaluate(assessments: &[SubconditionAssessment]) -> (Gate, Vec<Subcondition>) {
    let status_of = |wanted: Subcondition| {
        assessments
            .iter()
            .find(|a| a.subcondition == wanted)
            .map_or(ConditionStatus::NotAssessed, SubconditionAssessment::status)
    };

    let mut pending = Vec::new();
    let mut any_met = false;

    for subcondition in Subcondition::ALL {
        match status_of(subcondition) {
            ConditionStatus::Met => any_met = true,
            ConditionStatus::NotAssessed => pending.push(subcondition),
            ConditionStatus::NotMet => {}
        }
    }

    let gate = if any_met {
        Gate::Satisfied
    } else if pending.is_empty() {
        Gate::Refuted
    } else {
        Gate::Unknown
    };

    (gate, pending)
}
