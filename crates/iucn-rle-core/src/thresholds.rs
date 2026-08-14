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

const V2_2024_CRITERIA: &[CriterionThresholds] = &[
    CriterionThresholds {
        criterion: CriterionId::B1,
        breakpoints: B1_BREAKPOINTS,
        above_all: Category::Lc,
        requires_subcondition: true,
    },
    CriterionThresholds {
        criterion: CriterionId::B2,
        breakpoints: B2_BREAKPOINTS,
        above_all: Category::Lc,
        requires_subcondition: true,
    },
];

static V2_2024: ThresholdTable = ThresholdTable {
    guidelines_version: "2.0",
    guidelines_year: 2024,
    citation: "IUCN (2024) Guidelines for the application of IUCN Red List of \
               Ecosystems Categories and Criteria, Version 2.0",
    criteria: V2_2024_CRITERIA,
};

/// A complete set of thresholds for one edition of the Guidelines.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct ThresholdTable {
    guidelines_version: &'static str,
    guidelines_year: u32,
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
