//! IUCN Red List of Ecosystems risk categories.

use core::cmp::Ordering;
use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

/// An IUCN Red List of Ecosystems risk category.
///
/// Ordering is by risk, most threatened first, so `min()` over an iterator of
/// categories yields the most threatened one. That is the rule for combining
/// sub-criteria into a criterion, and criteria into an overall assessment.
///
/// ```
/// use iucn_rle_core::Category;
///
/// let overall = [Category::Lc, Category::En, Category::Vu].into_iter().min();
/// assert_eq!(overall, Some(Category::En));
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Category {
    /// Collapsed.
    Co,
    /// Critically Endangered.
    Cr,
    /// Endangered.
    En,
    /// Vulnerable.
    Vu,
    /// Near Threatened.
    Nt,
    /// Least Concern.
    Lc,
    /// Data Deficient.
    Dd,
    /// Not Evaluated.
    Ne,
}

impl Category {
    /// Every category, ordered from most to least threatened.
    pub const ALL: [Self; 8] = [
        Self::Co,
        Self::Cr,
        Self::En,
        Self::Vu,
        Self::Nt,
        Self::Lc,
        Self::Dd,
        Self::Ne,
    ];

    /// Risk rank, `0` being the most threatened.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Co => 0,
            Self::Cr => 1,
            Self::En => 2,
            Self::Vu => 3,
            Self::Nt => 4,
            Self::Lc => 5,
            Self::Dd => 6,
            Self::Ne => 7,
        }
    }

    /// The two-letter code used in assessments and reports.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Co => "CO",
            Self::Cr => "CR",
            Self::En => "EN",
            Self::Vu => "VU",
            Self::Nt => "NT",
            Self::Lc => "LC",
            Self::Dd => "DD",
            Self::Ne => "NE",
        }
    }

    /// The full name of the category.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Co => "Collapsed",
            Self::Cr => "Critically Endangered",
            Self::En => "Endangered",
            Self::Vu => "Vulnerable",
            Self::Nt => "Near Threatened",
            Self::Lc => "Least Concern",
            Self::Dd => "Data Deficient",
            Self::Ne => "Not Evaluated",
        }
    }

    /// Whether this category counts as threatened (CO, CR, EN, or VU).
    ///
    /// `DD` and `NE` are the absence of an assessment rather than a finding of
    /// safety, but they are not findings of threat either, so both are `false`.
    #[must_use]
    pub const fn is_threatened(self) -> bool {
        self.rank() <= Self::Vu.rank()
    }
}

impl Ord for Category {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for Category {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Returned when a string is not a recognised category code.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error("unknown IUCN RLE category code: {0:?}")]
pub struct ParseCategoryError(pub String);

impl FromStr for Category {
    type Err = ParseCategoryError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|c| c.code().eq_ignore_ascii_case(s))
            .ok_or_else(|| ParseCategoryError(s.to_owned()))
    }
}
