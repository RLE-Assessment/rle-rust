//! The 1% small-patch exclusion that produces the Criterion B2 number.
//!
//! IUCN (2024) Guidelines v2.0, §6.3.2, pp. 67-68. The protocol is specified
//! step by step, ending: "Calculate AOO by counting the number of cells with a
//! 'cumulative proportion' greater than 0.01 (i.e. exclude cells that in
//! combination account for up to 1% of the total mapped extent of the ecosystem
//! type)."
//!
//! Note this supersedes the Guidelines v1.1 rule, which excluded cells where the
//! ecosystem covered less than 1% of the *cell* (i.e. < 1 km²). That earlier rule
//! could exclude every cell when patches were small and widely separated.

use iucn_rle_core::aoo::one_percent_rule;

#[test]
fn no_cells_means_no_area_of_occupancy() {
    let result = one_percent_rule(&mut []);
    assert_eq!(result.aoo_cells, 0);
    assert_eq!(result.occupied_cell_count, 0);
}

#[test]
fn a_single_cell_survives() {
    // One cell holds 100% of the extent, so its cumulative proportion is 1.0.
    let result = one_percent_rule(&mut [0.42]);
    assert_eq!(result.aoo_cells, 1);
    assert_eq!(result.occupied_cell_count, 1);
}

#[test]
fn evenly_sized_cells_all_survive() {
    let result = one_percent_rule(&mut [0.5, 0.5, 0.5, 0.5]);
    assert_eq!(result.aoo_cells, 4);
}

#[test]
fn a_negligible_cell_is_excluded() {
    // The tiny cell accounts for well under 1% of the mapped extent, so it is
    // dropped: it contributes almost nothing to spreading risk, but would
    // otherwise inflate the count by a whole cell.
    let result = one_percent_rule(&mut [0.001, 1.0]);
    assert_eq!(
        result.aoo_cells, 1,
        "the negligible cell should be excluded"
    );
    assert_eq!(result.occupied_cell_count, 2, "but it is still occupied");
}

#[test]
fn exclusion_is_cumulative_not_per_cell() {
    // This is the whole point of the v2.0 protocol. Twenty cells at 0.04 each are
    // individually under 1% of the 4.0 total (each is 1%), but the rule excludes
    // the smallest cells only until their COMBINED share exceeds 1% — so most of
    // them survive. A per-cell rule would wrongly discard all twenty.
    let mut fractions = vec![0.04; 20];
    fractions.push(3.2);

    let result = one_percent_rule(&mut fractions);

    assert_eq!(result.occupied_cell_count, 21);
    assert!(
        result.aoo_cells >= 20,
        "a cumulative rule keeps nearly all of them, got {}",
        result.aoo_cells
    );
}

#[test]
fn the_boundary_is_strictly_greater_than_one_percent() {
    // A cell whose cumulative proportion is exactly 0.01 is excluded; the
    // Guidelines say "greater than 0.01", not "at least".
    let mut fractions = [1.0, 99.0];
    let result = one_percent_rule(&mut fractions);

    // 1.0 / 100.0 == 0.01 exactly, which is not > 0.01.
    assert_eq!(result.aoo_cells, 1);
}

#[test]
fn input_order_does_not_matter() {
    // The protocol sorts ascending, so the caller's order is irrelevant. Grid
    // iteration order must never change an assessment.
    let ascending = one_percent_rule(&mut [0.001, 0.5, 2.0, 7.0]);
    let descending = one_percent_rule(&mut [7.0, 2.0, 0.5, 0.001]);
    let shuffled = one_percent_rule(&mut [2.0, 0.001, 7.0, 0.5]);

    assert_eq!(ascending.aoo_cells, descending.aoo_cells);
    assert_eq!(ascending.aoo_cells, shuffled.aoo_cells);
}

#[test]
fn a_cell_with_no_extent_is_not_occupied() {
    // Zero-extent cells can appear from a grid intersection that touches only a
    // boundary. They occupy nothing and must not count.
    let result = one_percent_rule(&mut [0.0, 0.0, 5.0]);
    assert_eq!(result.occupied_cell_count, 1);
    assert_eq!(result.aoo_cells, 1);
}

#[test]
fn an_entirely_empty_map_does_not_divide_by_zero() {
    let result = one_percent_rule(&mut [0.0, 0.0]);
    assert_eq!(result.aoo_cells, 0);
    assert_eq!(result.occupied_cell_count, 0);
}

#[test]
fn the_margin_shows_how_close_the_count_was_to_changing() {
    // The cell count is an integer derived from a float comparison. Reporting the
    // distance from the 0.01 cutoff is what lets us claim the integer is robust to
    // floating-point differences between implementations, rather than hoping so.
    let comfortable = one_percent_rule(&mut [0.5, 0.5]);
    assert!(
        comfortable.threshold_margin > 0.1,
        "well clear of the cutoff, got {}",
        comfortable.threshold_margin
    );

    // Exactly on the boundary: margin is zero, and the answer is knife-edge.
    let knife_edge = one_percent_rule(&mut [1.0, 99.0]);
    assert!(
        knife_edge.threshold_margin < 1e-12,
        "sat on the cutoff, got {}",
        knife_edge.threshold_margin
    );
    assert!(knife_edge.near_boundary);
}

#[test]
fn total_extent_is_reported() {
    let result = one_percent_rule(&mut [1.5, 2.5, 6.0]);
    assert!((result.total_extent - 10.0).abs() < 1e-12);
}

#[test]
fn a_distribution_consistent_with_the_great_fish_thicket_counts() {
    // Guidelines Box 12, p.69 reports that 155 grid cells intersect the Great Fish
    // Thicket distribution and 145 remain after the exclusion.
    //
    // This is NOT a reproduction of that example. The Guidelines publish the two
    // counts but not the per-cell extents, so the actual case cannot be replicated
    // without the source distribution map — any fixture claiming to do so would be
    // fabricated. What this test does instead is construct a distribution that is
    // *consistent* with those counts and check the shape of the rule: 145
    // substantial cells and 10 small ones, sized so the ten together fall just
    // under 1% of the mapped extent while the eleventh cell clears it.
    //
    // Getting that sizing wrong is instructive. At 0.0001 each, the ten tiny cells
    // plus the first substantial one still sit under the cumulative 1% line, and
    // the answer is 144 — the exclusion is cumulative, so it can reach past the
    // cells you intended it to remove.
    //
    // Box 12's EOO figure (18,359.2 km²) *is* reproduced, in the shared
    // conformance corpus, because that one needs no underlying data.
    let mut extents = vec![1.0; 145];
    extents.extend(std::iter::repeat_n(0.1, 10));

    let result = one_percent_rule(&mut extents);

    assert_eq!(
        result.occupied_cell_count, 155,
        "cells intersecting the map"
    );
    assert_eq!(result.aoo_cells, 145, "cells after the 1% exclusion");
}
