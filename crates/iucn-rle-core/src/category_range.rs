//! A category with plausible bounds.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::Category;

/// A best-estimate category together with its plausible bounds.
///
/// IUCN assessments routinely report an outcome as a range — "EN (VU-CR)" —
/// because the underlying metric, the sub-conditions, or both are uncertain.
/// Neither `rle-python` nor `redlistr` can express that; this type can, and it
/// carries the uncertainty all the way through to the overall assessment.
///
/// ```
/// use iucn_rle_core::{Category, CategoryRange};
///
/// let range = CategoryRange::span(Category::En, Category::Vu, Category::Cr);
/// assert_eq!(range.to_string(), "EN (VU-CR)");
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CategoryRange {
    best: Category,
    /// The least threatened plausible outcome (higher rank number).
    plausible_least: Category,
    /// The most threatened plausible outcome (lower rank number).
    plausible_most: Category,
}

impl CategoryRange {
    /// A range with no uncertainty.
    #[must_use]
    pub const fn point(category: Category) -> Self {
        Self {
            best: category,
            plausible_least: category,
            plausible_most: category,
        }
    }

    /// A range spanning two bounds, given in either order.
    ///
    /// Callers should not have to know which of two categories is the more
    /// threatened, so the bounds are normalised by rank rather than by position.
    #[must_use]
    pub fn span(best: Category, one_bound: Category, other_bound: Category) -> Self {
        Self {
            best,
            plausible_least: one_bound.max(other_bound),
            plausible_most: one_bound.min(other_bound),
        }
    }

    /// The best estimate.
    #[must_use]
    pub const fn best(self) -> Category {
        self.best
    }

    /// The least threatened plausible outcome.
    #[must_use]
    pub const fn plausible_least(self) -> Category {
        self.plausible_least
    }

    /// The most threatened plausible outcome.
    #[must_use]
    pub const fn plausible_most(self) -> Category {
        self.plausible_most
    }

    /// Whether the bounds coincide, so the outcome is certain.
    #[must_use]
    pub fn is_point(self) -> bool {
        self.plausible_least == self.plausible_most
    }

    /// Combine several ranges into the most threatened, bound by bound.
    ///
    /// This is how sub-criteria roll up into a criterion and criteria into an
    /// overall assessment. Applying it to each bound separately means the
    /// uncertainty of the governing criterion survives into the headline result
    /// instead of being silently discarded.
    #[must_use]
    pub fn most_threatened(ranges: impl IntoIterator<Item = Self>) -> Option<Self> {
        ranges.into_iter().reduce(|a, b| Self {
            best: a.best.min(b.best),
            plausible_least: a.plausible_least.min(b.plausible_least),
            plausible_most: a.plausible_most.min(b.plausible_most),
        })
    }
}

impl fmt::Display for CategoryRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_point() {
            return f.write_str(self.best.code());
        }
        write!(
            f,
            "{} ({}-{})",
            self.best.code(),
            self.plausible_least.code(),
            self.plausible_most.code()
        )
    }
}
