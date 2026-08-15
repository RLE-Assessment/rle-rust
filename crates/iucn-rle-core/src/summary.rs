//! A flat, string-based view of an assessment, shaped for FFI.
//!
//! Rich Rust enums and nested structs do not cross a language boundary well, and
//! five bindings should not each invent their own flattening. This is that shape,
//! defined once: plain strings and booleans, serde-serialisable, with categories
//! already rendered so a caller compares `"EN (LC-EN)"` directly instead of
//! reassembling it from three enum fields.

use serde::{Deserialize, Serialize};

use crate::assessment::Note;
use crate::Assessment;

/// One sub-criterion, flattened.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CriterionSummary {
    /// The sub-criterion identifier, such as `"B1"`.
    pub criterion: String,
    /// The reportable category, such as `"EN (LC-EN)"`.
    pub category: String,
    /// What the thresholds alone said, before the sub-condition gate.
    pub threshold_category: Option<String>,
}

/// An assessment, flattened for crossing a language boundary.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Summary {
    /// The overall category, including plausible bounds.
    pub overall: String,
    /// The overall best estimate alone, without bounds.
    pub overall_best: String,
    /// Whether the best estimate is a threatened category.
    pub is_threatened: bool,
    /// Each sub-criterion.
    pub criteria: Vec<CriterionSummary>,
    /// Caveats, rendered for display.
    pub notes: Vec<String>,
    /// The Guidelines edition applied.
    pub guidelines_version: String,
    /// SHA-256 of the threshold table.
    pub thresholds_sha256: String,
    /// The engine that produced this.
    pub engine: String,
}

impl Assessment {
    /// Flatten this assessment for returning across a language boundary.
    #[must_use]
    pub fn summary(&self) -> Summary {
        Summary {
            overall: self.category().to_string(),
            overall_best: self.category().best().code().to_owned(),
            is_threatened: self.is_threatened(),
            criteria: self
                .results()
                .iter()
                .map(|r| CriterionSummary {
                    criterion: r.criterion().code().to_owned(),
                    category: r.category().to_string(),
                    threshold_category: r.threshold_category().map(|c| c.to_string()),
                })
                .collect(),
            notes: self.notes().iter().map(ToString::to_string).collect(),
            guidelines_version: self.provenance().guidelines_version().to_owned(),
            thresholds_sha256: self.provenance().thresholds_sha256().to_owned(),
            engine: self.provenance().engine().to_owned(),
        }
    }
}

impl core::fmt::Display for Note {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MetricAboveAllThresholds => f.write_str(
                "the metric exceeds every threshold, so this criterion is not triggered",
            ),
            Self::MetricMissing => {
                f.write_str("no metric was supplied, so this criterion could not be evaluated")
            }
            Self::SubconditionsRefuted => f.write_str(
                "every sub-condition was assessed and none is met, \
                 so this criterion is not triggered",
            ),
            Self::SubconditionsNotAssessed { pending } => {
                let letters: Vec<String> = pending.iter().map(|s| format!("({s})")).collect();
                write!(
                    f,
                    "sub-condition{} {} not assessed, so the listing is provisional",
                    if pending.len() == 1 { "" } else { "s" },
                    letters.join(", ")
                )
            }
            Self::LocationsInsufficientInformation => f.write_str(
                "threats exist but the number of threat-defined locations could not be \
                 assessed, so this sub-criterion is Data Deficient",
            ),
            Self::B3NotAssessed => f.write_str(
                "B3 requires both very few threat-defined locations and capability of rapid \
                 collapse; at least one limb is unassessed",
            ),
            Self::NearThreatenedMayApply {
                locations,
                max_locations,
            } => write!(
                f,
                "with {locations} threat-defined locations (<= {max_locations}), Near Threatened \
                 may apply under Guidelines Box 13 step 6(iv) if information is insufficient and \
                 less than 30% of the distribution is unthreatened"
            ),
        }
    }
}
