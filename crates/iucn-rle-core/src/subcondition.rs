//! Criterion B sub-conditions, modelled as data rather than prose.
//!
//! A listing under B1 or B2 requires the spatial threshold **and** at least one of
//! sub-conditions (a), (b) or (c) — IUCN (2024) Guidelines v2.0, Appendix 1
//! (Criteria v2.1), pp. 154-155. `rle-python` states this in a docstring, which no
//! caller can act on. Here it is typed, so the engine can distinguish "we know a
//! sub-condition holds" from "nobody has looked" and report the difference.
//!
//! The structure mirrors the criteria exactly:
//!
//! * **(a)** an observed or inferred continuing decline in **any of** (i) spatial
//!   extent, (ii) environmental quality, or (iii) biotic interactions;
//! * **(b)** observed or inferred threatening processes likely to cause continuing
//!   declines within the next 20 years;
//! * **(c)** the ecosystem exists at few threat-defined locations — a **category
//!   dependent** count, not a boolean: 1 for CR, <= 5 for EN, <= 10 for VU.

use core::str::FromStr;

use serde::{Deserialize, Serialize};

/// Returned when a string names no known sub-condition, status or aspect.
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

/// The three aspects of continuing decline under sub-condition (a).
///
/// Clause (a) is met if **any** of these is declining.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclineAspect {
    /// a(i): a measure of spatial extent appropriate to the ecosystem.
    SpatialExtent,
    /// a(ii): a measure of environmental quality appropriate to the characteristic biota.
    EnvironmentalQuality,
    /// a(iii): a measure of biotic interactions appropriate to the characteristic biota.
    BioticInteractions,
}

impl DeclineAspect {
    /// All three aspects, in Guidelines order.
    pub const ALL: [Self; 3] = [
        Self::SpatialExtent,
        Self::EnvironmentalQuality,
        Self::BioticInteractions,
    ];

    /// The roman numeral used in the criteria: `"i"`, `"ii"` or `"iii"`.
    #[must_use]
    pub const fn numeral(self) -> &'static str {
        match self {
            Self::SpatialExtent => "i",
            Self::EnvironmentalQuality => "ii",
            Self::BioticInteractions => "iii",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::SpatialExtent => 0,
            Self::EnvironmentalQuality => 1,
            Self::BioticInteractions => 2,
        }
    }
}

impl FromStr for DeclineAspect {
    type Err = ParseSubconditionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "i" | "spatial_extent" => Ok(Self::SpatialExtent),
            "ii" | "environmental_quality" => Ok(Self::EnvironmentalQuality),
            "iii" | "biotic_interactions" => Ok(Self::BioticInteractions),
            _ => Err(ParseSubconditionError {
                kind: "decline aspect",
                value: s.to_owned(),
                expected: "i|ii|iii, or spatial_extent|environmental_quality|biotic_interactions",
            }),
        }
    }
}

/// Whether a sub-condition holds.
///
/// `NotAssessed` is deliberately distinct from `NotMet`. Conflating them turns "we
/// have not checked" into "we checked and it is fine", which is the error this type
/// exists to prevent.
#[derive(Copy, Clone, Default, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionStatus {
    /// Assessed and found to hold.
    Met,
    /// Assessed and found not to hold.
    NotMet,
    /// Nobody has assessed this.
    #[default]
    NotAssessed,
}

/// What is known about the number of threat-defined locations, for clause (c) and B3.
///
/// The three non-count variants are not interchangeable. Box 13 step 5 is explicit:
/// "Where there are no plausible threats to the ecosystem type, subcriteria B1(c),
/// B2(c) and B3 are not met. This should be distinguished from cases in which there is
/// insufficient information to assess the number of threat-defined locations (i.e. a
/// Data Deficient outcome)."
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "count")]
pub enum ThreatLocations {
    /// A counted number of threat-defined locations.
    Count(u32),
    /// No plausible threats exist, so clause (c) and B3 are **not met**. A finding.
    NoPlausibleThreats,
    /// Threats exist but their extent cannot be assessed. Yields Data Deficient.
    InsufficientInformation,
    /// Nobody has looked.
    #[default]
    NotAssessed,
}

/// The evidence supporting Criterion B's sub-conditions.
///
/// Anything not supplied counts as [`ConditionStatus::NotAssessed`] or
/// [`ThreatLocations::NotAssessed`], which produces a bounded outcome rather than a
/// firm category.
///
/// ```
/// use iucn_rle_core::{ConditionStatus, DeclineAspect, Subconditions, ThreatLocations};
///
/// let subs = Subconditions::new()
///     .with_decline(DeclineAspect::SpatialExtent, ConditionStatus::Met)
///     .with_locations(ThreatLocations::Count(3));
/// ```
#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Subconditions {
    /// (a), indexed by [`DeclineAspect`].
    decline: [ConditionStatus; 3],
    /// (b).
    threatening_processes: ConditionStatus,
    /// (c), and the first limb of B3.
    locations: ThreatLocations,
    /// The second limb of B3: capable of collapse or becoming CR within a very short
    /// time period. Irrelevant to B1 and B2.
    capable_of_rapid_collapse: ConditionStatus,
}

impl Subconditions {
    /// No evidence supplied: every sub-condition is unassessed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the status of one aspect of continuing decline, clause (a).
    #[must_use]
    pub fn with_decline(mut self, aspect: DeclineAspect, status: ConditionStatus) -> Self {
        self.decline[aspect.index()] = status;
        self
    }

    /// Record the status of clause (b), threatening processes.
    #[must_use]
    pub const fn with_threatening_processes(mut self, status: ConditionStatus) -> Self {
        self.threatening_processes = status;
        self
    }

    /// Record what is known about threat-defined locations, clause (c) and B3.
    #[must_use]
    pub const fn with_locations(mut self, locations: ThreatLocations) -> Self {
        self.locations = locations;
        self
    }

    /// Record whether the ecosystem is capable of collapse or becoming CR within a very
    /// short time period — the second limb of B3.
    #[must_use]
    pub const fn with_capable_of_rapid_collapse(mut self, status: ConditionStatus) -> Self {
        self.capable_of_rapid_collapse = status;
        self
    }

    /// The status of one decline aspect.
    #[must_use]
    pub const fn decline(&self, aspect: DeclineAspect) -> ConditionStatus {
        self.decline[aspect.index()]
    }

    /// The status of clause (b).
    #[must_use]
    pub const fn threatening_processes(&self) -> ConditionStatus {
        self.threatening_processes
    }

    /// What is known about threat-defined locations.
    #[must_use]
    pub const fn locations(&self) -> ThreatLocations {
        self.locations
    }

    /// The status of B3's second limb.
    #[must_use]
    pub const fn capable_of_rapid_collapse(&self) -> ConditionStatus {
        self.capable_of_rapid_collapse
    }

    /// Clause (a): met if **any** aspect is declining; not met only once every aspect
    /// has been ruled out.
    #[must_use]
    pub fn continuing_decline(&self) -> ConditionStatus {
        if self.decline.contains(&ConditionStatus::Met) {
            ConditionStatus::Met
        } else if self.decline.iter().all(|s| *s == ConditionStatus::NotMet) {
            ConditionStatus::NotMet
        } else {
            ConditionStatus::NotAssessed
        }
    }

    /// Which clauses nobody has assessed, named as `"a"`, `"b"` and `"c"`.
    #[must_use]
    pub fn pending(&self) -> Vec<&'static str> {
        let mut pending = Vec::new();
        if self.continuing_decline() == ConditionStatus::NotAssessed {
            pending.push("a");
        }
        if self.threatening_processes == ConditionStatus::NotAssessed {
            pending.push("b");
        }
        if self.locations == ThreatLocations::NotAssessed {
            pending.push("c");
        }
        pending
    }
}

impl FromStr for ConditionStatus {
    type Err = ParseSubconditionError;

    /// Accepts `"met"`, `"not_met"` or `"not_assessed"`, case-insensitively.
    ///
    /// Deliberately does **not** accept `"unknown"`. It reads as a synonym for either
    /// `not_met` or `not_assessed` depending on who wrote it, and blurring those two is
    /// exactly the error this type exists to prevent.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
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
