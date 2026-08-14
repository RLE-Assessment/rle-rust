//! Assessment results: what was computed, what it means, and how to reproduce it.

use serde::{Deserialize, Serialize};

use crate::{Category, CategoryRange, CriterionId, Estimate, Subconditions};

/// A machine-readable caveat attached to a result.
///
/// Notes are data, never prose. Report renderers turn them into sentences; the
/// engine never emits a human-facing string that a caller has to parse back.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Note {
    /// The metric exceeded every threshold, so the criterion is not triggered.
    MetricAboveAllThresholds,
    /// No metric was supplied, so this sub-criterion could not be evaluated.
    MetricMissing,
    /// These sub-conditions were never assessed, so the outcome is a range.
    SubconditionsNotAssessed {
        /// Clause letters still awaiting assessment: `"a"`, `"b"`, `"c"`.
        pending: Vec<String>,
    },
    /// Every sub-condition was assessed and none is met.
    SubconditionsRefuted,
    /// Threats exist but their extent could not be assessed, so the sub-criterion is
    /// Data Deficient rather than a finding (Box 13, step 5).
    LocationsInsufficientInformation,
    /// B3's limbs were not both settled, so its outcome is a range.
    B3NotAssessed,
    /// The Box 13 step 6(iv) Near Threatened pathway may apply.
    ///
    /// That rule depends on judgements the engine cannot make — whether information is
    /// "insufficient", and whether less than 30% of the distribution is unthreatened —
    /// so it is surfaced for the assessor rather than computed.
    NearThreatenedMayApply {
        /// The number of threat-defined locations recorded.
        locations: u32,
        /// The count at or below which the pathway can be invoked.
        max_locations: u32,
    },
}

/// The outcome for one sub-criterion.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CriterionResult {
    criterion: CriterionId,
    metric: Option<Estimate>,
    threshold_category: Option<CategoryRange>,
    subconditions: Subconditions,
    category: CategoryRange,
    notes: Vec<Note>,
}

impl CriterionResult {
    pub(crate) fn new(
        criterion: CriterionId,
        metric: Option<Estimate>,
        threshold_category: Option<CategoryRange>,
        subconditions: Subconditions,
        category: CategoryRange,
        notes: Vec<Note>,
    ) -> Self {
        Self {
            criterion,
            metric,
            threshold_category,
            subconditions,
            category,
            notes,
        }
    }

    /// Which sub-criterion this is.
    #[must_use]
    pub const fn criterion(&self) -> CriterionId {
        self.criterion
    }

    /// The metric that was classified, if one was supplied.
    #[must_use]
    pub const fn metric(&self) -> Option<&Estimate> {
        self.metric.as_ref()
    }

    /// The outcome of the thresholds alone, before the sub-condition gate.
    ///
    /// Retained separately so a reviewer can see what the spatial thresholds
    /// said independently of whether the sub-conditions were established.
    #[must_use]
    pub const fn threshold_category(&self) -> Option<CategoryRange> {
        self.threshold_category
    }

    /// The sub-condition evidence that was supplied.
    #[must_use]
    pub const fn subconditions(&self) -> &Subconditions {
        &self.subconditions
    }

    /// The reportable outcome, after the sub-condition gate.
    #[must_use]
    pub const fn category(&self) -> CategoryRange {
        self.category
    }

    /// Machine-readable caveats.
    #[must_use]
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }
}

/// How a result was produced, in enough detail to reproduce it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Provenance {
    engine: String,
    guidelines_version: String,
    thresholds_sha256: String,
}

impl Provenance {
    pub(crate) fn new(guidelines_version: &str, thresholds_sha256: &str) -> Self {
        Self {
            engine: concat!("iucn-rle-core ", env!("CARGO_PKG_VERSION")).to_owned(),
            guidelines_version: guidelines_version.to_owned(),
            thresholds_sha256: thresholds_sha256.to_owned(),
        }
    }

    /// The engine name and version that produced the result.
    #[must_use]
    pub fn engine(&self) -> &str {
        &self.engine
    }

    /// The Guidelines edition whose thresholds were applied.
    #[must_use]
    pub fn guidelines_version(&self) -> &str {
        &self.guidelines_version
    }

    /// SHA-256 of the threshold table, so the exact numbers can be proven.
    #[must_use]
    pub fn thresholds_sha256(&self) -> &str {
        &self.thresholds_sha256
    }
}

/// A set of sub-criterion results and the category they imply.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Assessment {
    criteria: Vec<CriterionResult>,
    category: CategoryRange,
    notes: Vec<Note>,
    provenance: Provenance,
}

impl Assessment {
    pub(crate) fn new(
        criteria: Vec<CriterionResult>,
        category: CategoryRange,
        provenance: Provenance,
    ) -> Self {
        let notes = criteria
            .iter()
            .flat_map(|r| r.notes().iter().cloned())
            .collect();
        Self {
            criteria,
            category,
            notes,
            provenance,
        }
    }

    /// Every sub-criterion result.
    #[must_use]
    pub fn results(&self) -> &[CriterionResult] {
        &self.criteria
    }

    /// The result for one sub-criterion, if present.
    #[must_use]
    pub fn result(&self, criterion: CriterionId) -> Option<&CriterionResult> {
        self.criteria.iter().find(|r| r.criterion() == criterion)
    }

    /// The overall category: the most threatened across evaluated sub-criteria.
    #[must_use]
    pub const fn category(&self) -> CategoryRange {
        self.category
    }

    /// Whether the overall best estimate is a threatened category.
    #[must_use]
    pub fn is_threatened(&self) -> bool {
        self.category.best().is_threatened()
    }

    /// Every note raised by any sub-criterion.
    #[must_use]
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// How this result was produced.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Combine sub-criterion outcomes into an overall category.
///
/// Sub-criteria that could not be evaluated are skipped rather than dragging the
/// result down: one unevaluable metric must not mask a real finding from
/// another. If nothing at all could be evaluated, the result is `NE`.
pub(crate) fn overall(results: &[CriterionResult]) -> CategoryRange {
    let evaluated: Vec<CategoryRange> = results
        .iter()
        .map(CriterionResult::category)
        .filter(|c| c.best() != Category::Ne)
        .collect();

    // Data Deficient is the absence of an answer, not a low-risk answer. If any
    // sub-criterion produced a real category, that governs; DD only stands when
    // nothing else did.
    let decided: Vec<CategoryRange> = evaluated
        .iter()
        .copied()
        .filter(|c| c.best() != Category::Dd)
        .collect();

    let pool = if decided.is_empty() {
        evaluated
    } else {
        decided
    };

    CategoryRange::most_threatened(pool).unwrap_or_else(|| CategoryRange::point(Category::Ne))
}
