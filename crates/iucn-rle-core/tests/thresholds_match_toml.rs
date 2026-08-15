//! The compiled threshold constants must match the auditable TOML.
//!
//! `thresholds/iucn-rle-v2.0-2024.toml` exists so a reviewer who does not read
//! Rust can diff it against the published Guidelines, and so the Python, R,
//! Julia, and JavaScript distributions can ship identical bytes. That is only
//! worth anything if the file and the code cannot drift apart — which is what
//! this test enforces.
//!
//! Runtime never parses TOML; `toml` is a dev-dependency only.

// Exact float comparison is the entire point here: a threshold read from the
// TOML and the same threshold compiled into the binary must be bit-identical.
// An epsilon would let a genuinely different bound slip through.
#![allow(clippy::float_cmp)]

use iucn_rle_core::thresholds::V2_2024_TOML;
use iucn_rle_core::{Category, CriterionId, ThresholdTable};
use serde::Deserialize;

#[derive(Deserialize)]
struct File {
    guidelines_version: String,
    guidelines_year: u32,
    criteria_version: String,
    citation: String,
    criterion: Vec<Criterion>,
}

#[derive(Deserialize)]
struct Criterion {
    id: String,
    above_all: String,
    never_emit: Vec<String>,
    requires_any_subcondition: Vec<String>,
    breakpoints: Vec<Breakpoint>,
    location_bounds: Vec<LocationBound>,
}

#[derive(Deserialize)]
struct Breakpoint {
    max: f64,
    category: String,
}

#[derive(Deserialize)]
struct LocationBound {
    max_locations: u32,
    category: String,
}

fn parsed() -> File {
    toml::from_str(V2_2024_TOML).expect("threshold TOML must parse")
}

#[test]
fn metadata_matches() {
    let file = parsed();
    let table = ThresholdTable::v2_2024();

    assert_eq!(file.guidelines_version, table.guidelines_version());
    assert_eq!(file.guidelines_year, table.guidelines_year());
    assert_eq!(file.criteria_version, table.criteria_version());
    assert_eq!(file.citation, table.citation());
}

#[test]
fn every_criterion_in_the_file_is_compiled_in() {
    let file = parsed();
    let table = ThresholdTable::v2_2024();

    for criterion in &file.criterion {
        let id: CriterionId = criterion.id.parse().expect("valid criterion id");
        let rule = table
            .rule(id)
            .unwrap_or_else(|| panic!("{id} is in the TOML but not compiled in"));

        assert_eq!(
            rule.above_all,
            criterion.above_all.parse::<Category>().unwrap(),
            "{id}: above_all disagrees"
        );
        assert_eq!(
            rule.requires_subcondition,
            !criterion.requires_any_subcondition.is_empty(),
            "{id}: sub-condition requirement disagrees"
        );

        assert_eq!(
            rule.breakpoints.len(),
            criterion.breakpoints.len(),
            "{id}: breakpoint count disagrees"
        );
        for (compiled, from_file) in rule.breakpoints.iter().zip(&criterion.breakpoints) {
            assert_eq!(compiled.max, from_file.max, "{id}: bound disagrees");
            assert_eq!(
                compiled.category,
                from_file.category.parse::<Category>().unwrap(),
                "{id}: category at bound {} disagrees",
                from_file.max
            );
        }

        // Clause (c) is category dependent, so these bounds are as load-bearing as
        // the spatial ones and just as easy to get wrong.
        assert_eq!(
            rule.location_bounds.len(),
            criterion.location_bounds.len(),
            "{id}: location bound count disagrees"
        );
        for (compiled, from_file) in rule.location_bounds.iter().zip(&criterion.location_bounds) {
            assert_eq!(
                compiled.max_locations, from_file.max_locations,
                "{id}: location bound disagrees"
            );
            assert_eq!(
                compiled.category,
                from_file.category.parse::<Category>().unwrap(),
                "{id}: category at location bound {} disagrees",
                from_file.max_locations
            );
        }
    }
}

#[test]
fn every_compiled_criterion_is_in_the_file() {
    let file = parsed();
    for rule in ThresholdTable::v2_2024().criteria() {
        assert!(
            file.criterion.iter().any(|c| c.id == rule.criterion.code()),
            "{} is compiled in but absent from the TOML",
            rule.criterion
        );
    }
}

#[test]
fn no_criterion_can_emit_a_category_it_declares_it_never_emits() {
    let file = parsed();
    let table = ThresholdTable::v2_2024();

    for criterion in &file.criterion {
        let id: CriterionId = criterion.id.parse().unwrap();
        let rule = table.rule(id).unwrap();

        for banned in &criterion.never_emit {
            let banned: Category = banned.parse().unwrap();
            assert_ne!(
                rule.above_all, banned,
                "{id}: above_all is a banned category"
            );
            for breakpoint in rule.breakpoints {
                assert_ne!(
                    breakpoint.category, banned,
                    "{id}: breakpoint at {} yields banned category {banned}",
                    breakpoint.max
                );
            }
            // B3 declares it never emits CR or EN, per Section 6.3.3 p. 75.
            for bound in rule.location_bounds {
                assert_ne!(
                    bound.category, banned,
                    "{id}: location bound at {} yields banned category {banned}",
                    bound.max_locations
                );
            }
        }
    }
}

#[test]
fn breakpoints_are_ordered_and_strictly_increasing() {
    // Out-of-order bounds would silently make a breakpoint unreachable, because
    // classification takes the first match.
    for rule in ThresholdTable::v2_2024().criteria() {
        let bounds: Vec<f64> = rule.breakpoints.iter().map(|b| b.max).collect();
        let mut sorted = bounds.clone();
        sorted.sort_by(f64::total_cmp);
        sorted.dedup();
        assert_eq!(
            bounds, sorted,
            "{}: breakpoints must be strictly increasing",
            rule.criterion
        );
    }
}
