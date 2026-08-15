//! Versioned IUCN threshold tables.
//!
//! Thresholds are law, not logic. The auditable source of truth is
//! `thresholds/iucn-rle-v2.0-2024.toml`, which ships with every language
//! distribution so all of them can assert they agree. The constants below are
//! checked against that file by `tests/thresholds_match_toml.rs`, so the two
//! cannot drift, and the file's SHA-256 is computed at build time and pinned
//! into every assessment's provenance.

use crate::{Category, CategoryRange, CriterionId, Estimate};

/// One inclusive upper bound and the category met at it.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Breakpoint {
    /// Inclusive upper bound: a metric `<= max` meets `category`.
    pub max: f64,
    /// The category met at or below `max`.
    pub category: Category,
}

/// The maximum number of threat-defined locations that satisfies clause (c) at a
/// given category.
///
/// Clause (c) is **category dependent** — Appendix 1, pp. 154-155 reads "Ecosystem
/// exists at 1 threat-defined location" for CR, "<= 5" for EN and "<= 10" for VU. This
/// is why Criterion B must be evaluated per level rather than by picking a threshold
/// category from the metric and then applying a boolean gate.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct LocationBound {
    /// The category this bound applies at.
    pub category: Category,
    /// Inclusive maximum number of threat-defined locations.
    pub max_locations: u32,
}

/// The threshold rule for one sub-criterion.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct CriterionThresholds {
    /// Which sub-criterion this rule governs.
    pub criterion: CriterionId,
    /// Breakpoints in order; the first match wins.
    pub breakpoints: &'static [Breakpoint],
    /// The category for a metric above every breakpoint.
    pub above_all: Category,
    /// Whether a listing additionally requires sub-condition (a), (b), or (c).
    pub requires_subcondition: bool,
    /// Clause (c) bounds, per category. Empty when the sub-criterion has no clause (c).
    pub location_bounds: &'static [LocationBound],
}

/// Raised when a criterion has no threshold table.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error(
    "no threshold table for criterion {0} in IUCN RLE Guidelines v{1}; \
     it may not be transcribed yet, or may have no numeric thresholds at all"
)]
pub struct NoThresholdTable(pub CriterionId, pub &'static str);

/// The raw TOML source, shipped verbatim to every language distribution.
pub const V2_2024_TOML: &str = include_str!("../thresholds/iucn-rle-v2.0-2024.toml");

/// SHA-256 of [`V2_2024_TOML`], computed at build time.
pub const V2_2024_SHA256: &str = env!("IUCN_RLE_THRESHOLDS_V2_2024_SHA256");

// IUCN (2024) Guidelines v2.0, Section 6.2, p.66. Inclusive upper bounds.
const B1_BREAKPOINTS: &[Breakpoint] = &[
    Breakpoint {
        max: 2_000.0,
        category: Category::Cr,
    },
    Breakpoint {
        max: 20_000.0,
        category: Category::En,
    },
    Breakpoint {
        max: 50_000.0,
        category: Category::Vu,
    },
];

const B2_BREAKPOINTS: &[Breakpoint] = &[
    Breakpoint {
        max: 2.0,
        category: Category::Cr,
    },
    Breakpoint {
        max: 20.0,
        category: Category::En,
    },
    Breakpoint {
        max: 50.0,
        category: Category::Vu,
    },
];

// Clause (c), shared by B1 and B2: Appendix 1, pp. 154-155.
const CLAUSE_C_BOUNDS: &[LocationBound] = &[
    LocationBound {
        category: Category::Cr,
        max_locations: 1,
    },
    LocationBound {
        category: Category::En,
        max_locations: 5,
    },
    LocationBound {
        category: Category::Vu,
        max_locations: 10,
    },
];

/// B3's first limb: "Very small (generally fewer than 5 threat-defined locations)".
/// B3 can only ever yield VU (Section 6.3.3, p. 75).
const B3_BOUNDS: &[LocationBound] = &[LocationBound {
    category: Category::Vu,
    max_locations: 4,
}];

const V2_2024_CRITERIA: &[CriterionThresholds] = &[
    CriterionThresholds {
        criterion: CriterionId::B1,
        breakpoints: B1_BREAKPOINTS,
        above_all: Category::Lc,
        requires_subcondition: true,
        location_bounds: CLAUSE_C_BOUNDS,
    },
    CriterionThresholds {
        criterion: CriterionId::B2,
        breakpoints: B2_BREAKPOINTS,
        above_all: Category::Lc,
        requires_subcondition: true,
        location_bounds: CLAUSE_C_BOUNDS,
    },
    CriterionThresholds {
        criterion: CriterionId::B3,
        // B3 has no spatial metric; it is driven entirely by location count plus the
        // capable-of-rapid-collapse limb.
        breakpoints: &[],
        above_all: Category::Lc,
        requires_subcondition: false,
        location_bounds: B3_BOUNDS,
    },
];

static V2_2024: ThresholdTable = ThresholdTable {
    guidelines_version: "2.0",
    guidelines_year: 2024,
    criteria_version: "2.1",
    citation: "IUCN (2024) Guidelines for the application of IUCN Red List of \
               Ecosystems Categories and Criteria, Version 2.0",
    criteria: V2_2024_CRITERIA,
};

/// A complete set of thresholds for one edition of the Guidelines.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct ThresholdTable {
    guidelines_version: &'static str,
    guidelines_year: u32,
    criteria_version: &'static str,
    citation: &'static str,
    criteria: &'static [CriterionThresholds],
}

impl ThresholdTable {
    /// The thresholds from Guidelines v2.0 (2024).
    #[must_use]
    pub fn v2_2024() -> &'static Self {
        &V2_2024
    }

    /// Look up a table by Guidelines version string, such as `"2.0"`.
    #[must_use]
    pub fn for_version(version: &str) -> Option<&'static Self> {
        match version {
            "2.0" => Some(&V2_2024),
            _ => None,
        }
    }

    /// The Guidelines version this table encodes.
    #[must_use]
    pub const fn guidelines_version(&self) -> &'static str {
        self.guidelines_version
    }

    /// The publication year of the Guidelines edition.
    #[must_use]
    pub const fn guidelines_year(&self) -> u32 {
        self.guidelines_year
    }

    /// The version of the *Criteria* codified by this edition of the Guidelines.
    ///
    /// Deliberately distinct from [`Self::guidelines_version`]: Guidelines v2.0 (2024)
    /// codifies Criteria v2.1 (Appendix 1, p. 154).
    #[must_use]
    pub const fn criteria_version(&self) -> &'static str {
        self.criteria_version
    }

    /// The full citation for the Guidelines edition.
    #[must_use]
    pub const fn citation(&self) -> &'static str {
        self.citation
    }

    /// SHA-256 of the TOML source, for pinning into provenance.
    #[must_use]
    pub const fn sha256(&self) -> &'static str {
        V2_2024_SHA256
    }

    /// The rule for one sub-criterion, if this table has one.
    #[must_use]
    pub fn rule(&self, criterion: CriterionId) -> Option<&'static CriterionThresholds> {
        self.criteria.iter().find(|r| r.criterion == criterion)
    }

    /// Every sub-criterion with a table here.
    #[must_use]
    pub const fn criteria(&self) -> &'static [CriterionThresholds] {
        self.criteria
    }

    /// Classify a metric value against a sub-criterion's thresholds.
    ///
    /// Breakpoints are inclusive upper bounds evaluated in order, first match
    /// winning; a value above all of them yields the table's `above_all`
    /// category. `NT` is never produced, having no numeric breakpoint.
    ///
    /// # Errors
    ///
    /// Returns [`NoThresholdTable`] if this criterion has no thresholds.
    pub fn classify(
        &self,
        criterion: CriterionId,
        value: f64,
    ) -> Result<Category, NoThresholdTable> {
        let rule = self
            .rule(criterion)
            .ok_or(NoThresholdTable(criterion, self.guidelines_version))?;

        Ok(rule
            .breakpoints
            .iter()
            .find(|b| value <= b.max)
            .map_or(rule.above_all, |b| b.category))
    }

    /// The categories a sub-criterion can reach, most threatened first.
    ///
    /// Criterion B is evaluated level by level because clause (c) is category
    /// dependent, so callers need the ladder rather than a single answer.
    #[must_use]
    pub fn levels(&self, criterion: CriterionId) -> Vec<Category> {
        self.rule(criterion).map_or_else(Vec::new, |rule| {
            let mut levels: Vec<Category> = rule
                .breakpoints
                .iter()
                .map(|b| b.category)
                .chain(rule.location_bounds.iter().map(|b| b.category))
                .collect();
            levels.sort_unstable();
            levels.dedup();
            levels
        })
    }

    /// Whether `value` meets this sub-criterion's spatial threshold at `level`.
    ///
    /// A sub-criterion with no breakpoints (B3) has no spatial threshold, so every
    /// level passes and the outcome rests entirely on the sub-conditions.
    #[must_use]
    pub fn metric_meets(&self, criterion: CriterionId, level: Category, value: f64) -> bool {
        self.rule(criterion).is_some_and(|rule| {
            rule.breakpoints.is_empty()
                || rule
                    .breakpoints
                    .iter()
                    .any(|b| b.category == level && value <= b.max)
        })
    }

    /// The inclusive maximum number of threat-defined locations satisfying clause (c)
    /// at `level`, if this sub-criterion has a clause (c) at that level.
    #[must_use]
    pub fn max_locations(&self, criterion: CriterionId, level: Category) -> Option<u32> {
        self.rule(criterion).and_then(|rule| {
            rule.location_bounds
                .iter()
                .find(|b| b.category == level)
                .map(|b| b.max_locations)
        })
    }

    /// Classify an estimate, carrying its uncertainty into the category.
    ///
    /// Both metric bounds are classified and the results normalised by risk
    /// rank, so this works whether smaller or larger values are the more
    /// threatening — the caller does not have to know the direction.
    ///
    /// # Errors
    ///
    /// Returns [`NoThresholdTable`] if this criterion has no thresholds.
    pub fn classify_estimate(
        &self,
        criterion: CriterionId,
        estimate: &Estimate,
    ) -> Result<CategoryRange, NoThresholdTable> {
        let best = self.classify(criterion, estimate.best())?;
        if estimate.is_point() {
            return Ok(CategoryRange::point(best));
        }

        let (lower, upper) = estimate.bounds();
        Ok(CategoryRange::span(
            best,
            self.classify(criterion, lower)?,
            self.classify(criterion, upper)?,
        ))
    }
}
