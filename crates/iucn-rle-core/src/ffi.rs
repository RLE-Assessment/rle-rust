//! The entry point every language binding calls.
//!
//! Python, R, JavaScript, Julia, and the CLI all need to turn loosely-typed
//! input — optional floats and pairs of strings — into the strongly-typed core
//! API, and to flatten the result back out. Doing that once here rather than
//! five times in five languages is what keeps the bindings thin enough to trust,
//! and it means an input-validation bug can only exist in one place.
//!
//! Errors are returned as plain `String` deliberately: a rich error type buys
//! nothing across an FFI boundary that can only carry a message anyway.

use serde::{Deserialize, Serialize};

use crate::{
    criterion_b, Basis, ConditionStatus, DeclineAspect, Estimate, Subconditions, Summary,
    ThreatLocations, ThresholdTable,
};

/// A metric value with optional plausible bounds, as supplied by a caller.
#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct MetricInput {
    /// The best estimate.
    pub best: f64,
    /// An optional plausible lower bound.
    pub lower: Option<f64>,
    /// An optional plausible upper bound.
    pub upper: Option<f64>,
}

impl MetricInput {
    /// A metric with no stated uncertainty.
    #[must_use]
    pub const fn point(best: f64) -> Self {
        Self {
            best,
            lower: None,
            upper: None,
        }
    }

    /// A metric with plausible bounds.
    #[must_use]
    pub const fn bounded(best: f64, lower: f64, upper: f64) -> Self {
        Self {
            best,
            lower: Some(lower),
            upper: Some(upper),
        }
    }

    fn into_estimate(self) -> Estimate {
        // Bindings do not carry an evidence basis yet; `Estimated` is the
        // neutral default and is recorded so it can be overridden later without
        // changing the shape of this type.
        match (self.lower, self.upper) {
            (Some(lower), Some(upper)) => {
                Estimate::bounded(self.best, lower, upper, Basis::Estimated)
            }
            _ => Estimate::point(self.best, Basis::Estimated),
        }
    }
}

/// A clause and its status, as supplied by a caller.
///
/// `sub` accepts `"a"` or `"b"`, the decline aspects `"a.i"` / `"a.ii"` / `"a.iii"`,
/// or `"b3_rapid_collapse"` for B3's second limb. Clause (c) is **not** given here — it
/// is a count of threat-defined locations, supplied via
/// [`SubconditionsInput::locations`].
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SubconditionInput {
    /// Which clause this concerns.
    pub sub: String,
    /// `"met"`, `"not_met"`, or `"not_assessed"`.
    pub status: String,
}

/// All Criterion B evidence, as supplied by a caller.
#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SubconditionsInput {
    /// Clause (a) aspects, clause (b), and B3's rapid-collapse limb.
    pub clauses: Vec<SubconditionInput>,
    /// Clause (c): the number of threat-defined locations, when counted.
    pub locations: Option<u32>,
    /// No plausible threats exist, so clause (c) and B3 are **not met** — a finding,
    /// distinct from an absent count, which means nobody looked.
    pub no_plausible_threats: bool,
    /// Threats exist but their extent cannot be assessed: Data Deficient.
    pub locations_insufficient_information: bool,
}

impl SubconditionsInput {
    fn into_subconditions(self) -> Result<Subconditions, String> {
        let mut subs = Subconditions::new();

        for clause in &self.clauses {
            let status: ConditionStatus = clause.status.parse().map_err(|e| format!("{e}"))?;
            let key = clause.sub.trim().to_ascii_lowercase();
            subs = match key.as_str() {
                // A bare "a" sets every aspect, for callers that do not distinguish them.
                "a" | "continuing_decline" => DeclineAspect::ALL
                    .into_iter()
                    .fold(subs, |acc, aspect| acc.with_decline(aspect, status)),
                "b" | "threatening_processes" => subs.with_threatening_processes(status),
                "b3_rapid_collapse" | "capable_of_rapid_collapse" => {
                    subs.with_capable_of_rapid_collapse(status)
                }
                "c" | "few_locations" => {
                    return Err("clause (c) is a count of threat-defined locations, not a \
                                status; supply it as `locations` instead"
                        .to_owned())
                }
                other => {
                    let aspect: DeclineAspect = other
                        .strip_prefix("a.")
                        .unwrap_or(other)
                        .parse()
                        .map_err(|_| {
                        format!(
                            "unrecognised clause: {:?}; expected \
                                     a|b|a.i|a.ii|a.iii|b3_rapid_collapse",
                            clause.sub
                        )
                    })?;
                    subs.with_decline(aspect, status)
                }
            };
        }

        let locations = match (
            self.locations,
            self.no_plausible_threats,
            self.locations_insufficient_information,
        ) {
            (Some(_), true, _) | (Some(_), _, true) | (None, true, true) => {
                return Err(
                    "threat-defined locations are over-specified: give at most one of \
                            a count, no_plausible_threats, or \
                            locations_insufficient_information"
                        .to_owned(),
                )
            }
            (Some(n), _, _) => ThreatLocations::Count(n),
            (None, true, false) => ThreatLocations::NoPlausibleThreats,
            (None, false, true) => ThreatLocations::InsufficientInformation,
            (None, false, false) => ThreatLocations::NotAssessed,
        };

        Ok(subs.with_locations(locations))
    }
}

/// Assess Criterion B from loosely-typed input.
///
/// Anything not supplied counts as **not assessed**, which produces a provisional range
/// rather than a definite category.
///
/// # Errors
///
/// Returns a human-readable message if a clause name or status is not recognised, or if
/// threat-defined locations are over-specified. Absent metrics are not an error; they
/// yield `NE`.
pub fn criterion_b_from_parts(
    eoo_km2: Option<MetricInput>,
    aoo_cells: Option<MetricInput>,
    subconditions: SubconditionsInput,
) -> Result<Summary, String> {
    let subs = subconditions.into_subconditions()?;

    let assessment = criterion_b(
        eoo_km2.map(MetricInput::into_estimate),
        aoo_cells.map(MetricInput::into_estimate),
        &subs,
        ThresholdTable::v2_2024(),
    )
    .map_err(|e| format!("{e}"))?;

    Ok(assessment.summary())
}
