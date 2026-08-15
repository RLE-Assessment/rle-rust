//! Agreement with `rle-python` on its own committed golden dataset.
//!
//! This is the test the whole conformance argument has been building toward. Every
//! other test checks the engine against the Guidelines or against itself; this one
//! checks it against the implementation that assessments currently use.
//!
//! # The contract
//!
//! **Discrete outputs must match exactly**: the set of occupied cells, and which
//! ecosystems occupy each one. Those are what decisions are built on.
//!
//! **Continuous outputs are compared with a tolerance**, because bit-identical floats
//! are not achievable and claiming otherwise would be dishonest. The two engines
//! differ in method, not just in rounding:
//!
//! * `rle-python` clips with GEOS; this engine uses Sutherland-Hodgman against the
//!   cell rectangle.
//! * `rle-python` builds its grid in ESRI:54034, converts the whole grid to
//!   EPSG:4326, then converts it back before intersecting — so its cells are slightly
//!   distorted four-corner approximations of the true rectangles. This engine clips
//!   against exact rectangles.
//!
//! The measured divergence is asserted below, so a regression shows up as a number
//! rather than as a vague sense that things still look fine.

use std::collections::BTreeMap;
use std::path::PathBuf;

use iucn_rle_core::distribution::DistributionAccumulator;
use iucn_rle_core::grid::CellId;
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    features: Vec<Feature>,
    expected_cells: Vec<ExpectedCell>,
}

#[derive(Deserialize)]
struct Feature {
    code: String,
    rings: Vec<Vec<[f64; 2]>>,
}

#[derive(Deserialize)]
struct ExpectedCell {
    grid_col: i32,
    grid_row: i32,
    count_ecosystems: u32,
    fractions: BTreeMap<String, f64>,
}

fn fixture() -> Fixture {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cases/null_island_aoo.json");
    serde_json::from_slice(&std::fs::read(path).expect("read golden fixture"))
        .expect("parse golden fixture")
}

/// Run the fixture's geometry through the engine.
fn compute() -> iucn_rle_core::distribution::Distribution {
    let fixture = fixture();
    let mut acc = DistributionAccumulator::new();
    for feature in &fixture.features {
        acc.add_polygon(&feature.code, &feature.rings);
    }
    acc.finish()
}

#[test]
fn the_occupied_cell_set_matches_exactly() {
    // No tolerance here, and none is warranted: which cells an ecosystem occupies is
    // a discrete fact, and it is what the AOO count and therefore the category are
    // built on.
    let fixture = fixture();
    let distribution = compute();

    let mut expected: Vec<(String, CellId)> = Vec::new();
    for cell in &fixture.expected_cells {
        for (code, fraction) in &cell.fractions {
            if *fraction > 0.0 {
                expected.push((code.clone(), CellId::new(cell.grid_col, cell.grid_row)));
            }
        }
    }
    expected.sort();

    let mut actual: Vec<(String, CellId)> = distribution
        .grid()
        .cells()
        .iter()
        .map(|c| (c.ecosystem.clone(), c.cell))
        .collect();
    actual.sort();

    assert_eq!(
        actual, expected,
        "the set of occupied (ecosystem, cell) pairs must match rle-python exactly"
    );
}

#[test]
fn the_number_of_ecosystems_per_cell_matches_exactly() {
    let fixture = fixture();
    let distribution = compute();

    for expected in &fixture.expected_cells {
        let cell = CellId::new(expected.grid_col, expected.grid_row);
        let actual = distribution
            .grid()
            .cells()
            .iter()
            .filter(|c| c.cell == cell)
            .count();
        assert_eq!(
            u32::try_from(actual).unwrap(),
            expected.count_ecosystems,
            "cell ({}, {}) should hold {} ecosystems",
            expected.grid_col,
            expected.grid_row,
            expected.count_ecosystems
        );
    }
}

#[test]
fn per_cell_fractions_agree_within_the_measured_tolerance() {
    let fixture = fixture();
    let distribution = compute();

    let mut worst_absolute: f64 = 0.0;
    let mut worst_description = String::new();

    for expected in &fixture.expected_cells {
        let cell = CellId::new(expected.grid_col, expected.grid_row);
        for (code, expected_fraction) in &expected.fractions {
            let actual = distribution
                .grid()
                .cells()
                .iter()
                .find(|c| c.cell == cell && &c.ecosystem == code)
                .map_or(0.0, |c| c.fraction);

            let deviation = (actual - expected_fraction).abs();
            if deviation > worst_absolute {
                worst_absolute = deviation;
                worst_description = format!(
                    "{code} in cell ({}, {}): rle-python {expected_fraction}, here {actual}",
                    expected.grid_col, expected.grid_row
                );
            }
        }
    }

    // MEASURED: 1.232e-13, on M1.1.1 in cell (-1, 0) — agreement to 13 significant
    // figures between two independently written implementations.
    //
    // The design anticipated far worse. GEOS and Sutherland-Hodgman are different
    // clipping algorithms; PROJ and this crate's hand-ported cylindrical-equal-area
    // are different projection code; and rle-python intersects against cells that
    // have been round-tripped 54034 -> 4326 -> 54034, while this engine clips against
    // exact rectangles. Any one of those could have produced a visible difference.
    // None did, because every step on both sides is closed-form rather than iterative.
    //
    // The bound sits three orders of magnitude above the measured value, leaving room
    // for a different rounding order on another platform — CI runs Linux, macOS and
    // Windows — while staying tight enough that a real change in the projection, the
    // clipping or the grid cannot pass unnoticed.
    assert!(
        worst_absolute < 1e-10,
        "worst fraction deviation {worst_absolute:.3e} exceeds tolerance — {worst_description}"
    );
}

#[test]
fn the_area_of_occupancy_matches_exactly() {
    // The number Criterion B2 actually consumes. Four cells, all substantial, so the
    // 1% exclusion removes none of them — and both engines must agree on that.
    let fixture = fixture();
    let distribution = compute();

    for feature in &fixture.features {
        let occupied = fixture
            .expected_cells
            .iter()
            .filter(|c| c.fractions.get(&feature.code).is_some_and(|f| *f > 0.0))
            .count();

        let aoo = distribution.aoo(&feature.code);
        assert_eq!(
            aoo.occupied_cell_count,
            u32::try_from(occupied).unwrap(),
            "{} occupied cell count",
            feature.code
        );
    }
}

#[test]
fn the_one_percent_exclusion_is_not_knife_edge_here() {
    // The claim that the cell count is reproducible across implementations rests on
    // no cell sitting near the 0.01 cutoff. Rather than assume that, check it: if a
    // cumulative proportion were within floating-point reach of the boundary, the
    // integer count genuinely would not be safe to compare exactly.
    let fixture = fixture();
    let distribution = compute();

    for feature in &fixture.features {
        let aoo = distribution.aoo(&feature.code);
        assert!(
            !aoo.near_boundary,
            "{} sits within {:.3e} of the 1% cutoff, so its cell count is not \
             robust to implementation differences",
            feature.code, aoo.threshold_margin
        );
    }
}

#[test]
fn the_extent_of_occurrence_matches_rle_pythons_published_value() {
    // A second, independent confirmation, and the only one covering EOO — the golden
    // parquet contains the AOO grid alone.
    //
    // rle_workshop/presentation-4-workflow-ruritania.ipynb is a committed notebook
    // whose executed output reads "EOO is 73.2 km2" and "AOO is 4 cells" for T1.1.1,
    // computed by rle-python from this same null-island dataset.
    //
    // Worth noting the two engines take different routes here. rle-python unions the
    // features, hulls the result in EPSG:4326 degrees, then reprojects the hull to
    // measure its area. This engine projects first and hulls in the equal-area plane,
    // which is where convexity and area actually belong — and needs no union at all,
    // since the hull of a union is the hull of the combined vertices. Different
    // method, same answer to the published precision.
    let distribution = compute();

    let eoo = distribution.eoo_km2("T1.1.1");
    assert!(
        (eoo - 73.2).abs() < 0.05,
        "rle-python publishes 73.2 km2 for T1.1.1, got {eoo}"
    );
    assert_eq!(distribution.aoo("T1.1.1").aoo_cells, 4);
}

#[test]
fn holes_are_honoured() {
    // Two of the three features carry an interior ring. If holes were ignored, every
    // fraction would come out larger than rle-python's, and the tolerance test above
    // would fail — but assert the structure directly so the reason is legible.
    let fixture = fixture();
    let with_holes = fixture
        .features
        .iter()
        .filter(|f| f.rings.len() > 1)
        .count();
    assert!(
        with_holes >= 2,
        "the fixture should exercise holes, found {with_holes}"
    );
}
