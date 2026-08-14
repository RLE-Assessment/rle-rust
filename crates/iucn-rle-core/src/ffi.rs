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
    criterion_b, Basis, ConditionStatus, Estimate, Subcondition, SubconditionAssessment, Summary,
    ThresholdTable,
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

/// A sub-condition and its status, as supplied by a caller.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SubconditionInput {
    /// `"a"`, `"b"`, `"c"`, or the `snake_case` name.
    pub sub: String,
    /// `"met"`, `"not_met"`, or `"not_assessed"`.
    pub status: String,
}

/// Assess Criterion B from loosely-typed input.
///
/// Sub-conditions absent from `subconditions` count as **not assessed**, which
/// is what produces a provisional range rather than a definite category.
///
/// # Errors
///
/// Returns a human-readable message if a sub-condition letter or status is not
/// recognised. Absent metrics are not an error; they yield `NE`.
pub fn criterion_b_from_parts(
    eoo_km2: Option<MetricInput>,
    aoo_cells: Option<MetricInput>,
    subconditions: &[SubconditionInput],
) -> Result<Summary, String> {
    let mut parsed = Vec::with_capacity(subconditions.len());
    for input in subconditions {
        let subcondition: Subcondition = input.sub.parse().map_err(|e| format!("{e}"))?;
        let status: ConditionStatus = input.status.parse().map_err(|e| format!("{e}"))?;
        parsed.push(SubconditionAssessment::new(subcondition, status));
    }

    let assessment = criterion_b(
        eoo_km2.map(MetricInput::into_estimate),
        aoo_cells.map(MetricInput::into_estimate),
        &parsed,
        ThresholdTable::v2_2024(),
    )
    .map_err(|e| format!("{e}"))?;

    Ok(assessment.summary())
}
