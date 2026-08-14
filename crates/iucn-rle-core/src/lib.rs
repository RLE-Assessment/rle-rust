//! Pure, synchronous IUCN Red List of Ecosystems assessment calculations.
//!
//! This crate is deliberately *sans-IO*: it never opens a socket, reads a file, or asks
//! the clock for the time. Every input arrives as plain data and every output is plain
//! data. Two things fall out of that constraint:
//!
//! * It compiles for `wasm32-unknown-unknown` unchanged, which is what makes the browser
//!   binding possible at all.
//! * Its public API and its test surface are the same surface, so the cross-language
//!   fixture corpus can exercise all of it from Rust, Python, R, and JavaScript.
//!
//! Anything that needs bytes from a URL lives in `iucn-rle-io` and `iucn-rle-engine`.
//!
//! # Status
//!
//! M1. Category, threshold, and Criterion B types; no geometry and no I/O yet.

pub mod aoo;
pub mod assessment;
pub mod category;
pub mod category_range;
pub mod criteria;
pub mod criterion;
pub mod eoo;
pub mod estimate;
pub mod ffi;
pub mod geometry;
pub mod grid;
pub mod subcondition;
pub mod summary;
pub mod thresholds;

pub use assessment::{Assessment, CriterionResult, Note, Provenance};
pub use category::{Category, ParseCategoryError};
pub use category_range::CategoryRange;
pub use criteria::criterion_b;
pub use criterion::{CriterionId, ParseCriterionError};
pub use estimate::{Basis, Estimate};
pub use subcondition::{
    ConditionStatus, DeclineAspect, ParseSubconditionError, Subconditions, ThreatLocations,
};
pub use summary::{CriterionSummary, Summary};
pub use thresholds::{NoThresholdTable, ThresholdTable};

/// The version of this crate, as reported to every language binding.
///
/// Bindings surface this verbatim so a user can confirm which engine produced a number.
/// It is also the first field of the provenance record attached to every assessment.
///
/// ```
/// assert!(!iucn_rle_core::version().is_empty());
/// ```
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_matches_cargo_manifest() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn version_is_semver_triple() {
        let parts: Vec<&str> = version().split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "expected MAJOR.MINOR.PATCH, got {:?}",
            version()
        );
        for part in parts {
            assert!(
                part.chars().all(|c| c.is_ascii_digit()),
                "non-numeric version component in {:?}",
                version()
            );
        }
    }
}
