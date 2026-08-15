//! Metric estimates with plausible bounds and an evidence basis.

use serde::{Deserialize, Serialize};

/// How a value was arrived at.
///
/// IUCN Guidelines v2.0 distinguish these because the same number carries very
/// different weight depending on its provenance, and an assessment is expected
/// to state which applies.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Basis {
    /// Directly measured.
    Observed,
    /// Calculated from measurements, with some interpolation.
    Estimated,
    /// Extrapolated forward in time from measurements.
    Projected,
    /// Derived indirectly from related measurements.
    Inferred,
    /// Based on circumstantial evidence or expert judgement.
    Suspected,
}

/// A metric value with optional plausible bounds.
///
/// The bounds are what let an assessment report `EN (VU-CR)` rather than a bare
/// `EN` that overstates confidence.
///
/// ```
/// use iucn_rle_core::{Basis, Estimate};
///
/// let eoo = Estimate::bounded(20_000.0, 15_000.0, 25_000.0, Basis::Inferred);
/// assert_eq!(eoo.best(), 20_000.0);
/// ```
#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Estimate {
    best: f64,
    lower: Option<f64>,
    upper: Option<f64>,
    basis: Basis,
}

impl Estimate {
    /// An estimate with no stated uncertainty.
    #[must_use]
    pub const fn point(best: f64, basis: Basis) -> Self {
        Self {
            best,
            lower: None,
            upper: None,
            basis,
        }
    }

    /// An estimate with plausible bounds, given in either order.
    #[must_use]
    pub fn bounded(best: f64, one_bound: f64, other_bound: f64, basis: Basis) -> Self {
        Self {
            best,
            lower: Some(one_bound.min(other_bound)),
            upper: Some(one_bound.max(other_bound)),
            basis,
        }
    }

    /// The best estimate.
    #[must_use]
    pub const fn best(&self) -> f64 {
        self.best
    }

    /// The lower plausible bound, if stated.
    #[must_use]
    pub const fn lower(&self) -> Option<f64> {
        self.lower
    }

    /// The upper plausible bound, if stated.
    #[must_use]
    pub const fn upper(&self) -> Option<f64> {
        self.upper
    }

    /// The evidence basis for this value.
    #[must_use]
    pub const fn basis(&self) -> Basis {
        self.basis
    }

    /// Whether no uncertainty was stated.
    #[must_use]
    pub const fn is_point(&self) -> bool {
        self.lower.is_none() && self.upper.is_none()
    }

    /// The bounds to classify, falling back to the best estimate when absent.
    pub(crate) fn bounds(&self) -> (f64, f64) {
        (
            self.lower.unwrap_or(self.best),
            self.upper.unwrap_or(self.best),
        )
    }
}
