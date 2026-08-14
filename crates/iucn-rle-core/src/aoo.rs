//! Area of occupancy: the Criterion B2 metric.
//!
//! AOO is a **count of occupied 10 × 10 km grid cells**, not an area. The grid size
//! is fixed by the Guidelines rather than chosen per assessment, because extent
//! estimates are extremely sensitive to the resolution of the source map and a
//! standard grain is what makes assessments comparable (§6.3.2, p. 67).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::geometry::{clipped_area, ring_area};
use crate::grid::{CellId, AOO_CELL_SIZE_M};

/// Area of one grid cell, in m².
const CELL_AREA_M2: f64 = AOO_CELL_SIZE_M * AOO_CELL_SIZE_M;

/// Neumaier compensated summation.
///
/// Cell extents arrive in whatever order the grid was traversed, and a naive sum
/// of many small values next to a few large ones loses low-order bits differently
/// depending on that order. Since the result feeds an integer cell count through a
/// threshold comparison, an order-dependent total could change an assessment.
/// Compensating makes the total order-independent to within about one ulp.
#[derive(Default, Clone, Copy, Debug)]
struct Neumaier {
    sum: f64,
    compensation: f64,
}

impl Neumaier {
    #[allow(clippy::float_cmp)]
    fn add(&mut self, value: f64) {
        let t = self.sum + value;
        if self.sum.abs() >= value.abs() {
            self.compensation += (self.sum - t) + value;
        } else {
            self.compensation += (value - t) + self.sum;
        }
        self.sum = t;
    }

    fn value(self) -> f64 {
        self.sum + self.compensation
    }
}

/// The outcome of applying the 1% small-patch exclusion.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct OnePercentResult {
    /// **The Criterion B2 number**: cells remaining after the exclusion.
    pub aoo_cells: u32,
    /// Every cell containing any of the ecosystem, before the exclusion.
    ///
    /// Deliberately a different name from [`Self::aoo_cells`]. `rle-python` calls
    /// both of these "AOO", and they are not the same number.
    pub occupied_cell_count: u32,
    /// Total mapped extent across all occupied cells, in the caller's units.
    pub total_extent: f64,
    /// Smallest distance from any cell's cumulative proportion to the 0.01 cutoff.
    ///
    /// This is what makes the cell count's reproducibility checkable rather than
    /// assumed: if the nearest cell sits far from the cutoff, no plausible
    /// floating-point difference between implementations can change the integer.
    pub threshold_margin: f64,
    /// Whether some cell sat close enough to the cutoff that the count is not
    /// robust, and a human should look.
    pub near_boundary: bool,
}

/// The cutoff from the Guidelines: cells are kept when their cumulative
/// proportion is **greater than** this, not at least this.
const CUTOFF: f64 = 0.01;

/// Below this margin the integer count is not safely reproducible across
/// implementations, and the result is flagged for a human.
const NEAR_BOUNDARY_MARGIN: f64 = 1e-9;

/// Apply the 1% small-patch exclusion to per-cell extents.
///
/// `extents` holds the amount of the ecosystem in each occupied cell, in any
/// consistent unit — km², a fraction of a cell, anything proportional. Only ratios
/// matter. The slice is sorted in place, so the caller's order is irrelevant.
///
/// The protocol, verbatim from §6.3.2 (pp. 67–68):
///
/// 1. Intersect the AOO grid with the ecosystem's distribution map.
/// 2. Calculate extent in each grid cell, and sum them for the total.
/// 3. Arrange cells in ascending order by extent, smaller first.
/// 4. Take the cumulative sum.
/// 5. Divide by the total to get the cumulative proportion.
/// 6. Count the cells whose cumulative proportion is greater than 0.01.
///
/// The exclusion is **cumulative, not per cell**. Many individually-small cells
/// survive as long as their combined share passes 1%. The Guidelines v1.1 rule —
/// drop any cell where the ecosystem covers under 1 km² — is explicitly superseded,
/// because it could exclude every cell when patches were small and widely separated.
///
/// ```
/// use iucn_rle_core::aoo::one_percent_rule;
///
/// // One negligible patch alongside a substantial one.
/// let result = one_percent_rule(&mut [0.001, 1.0]);
/// assert_eq!(result.aoo_cells, 1);           // the Criterion B2 number
/// assert_eq!(result.occupied_cell_count, 2); // cells actually touched
/// ```
#[must_use]
pub fn one_percent_rule(extents: &mut [f64]) -> OnePercentResult {
    // A cell can be reported with zero extent when a grid intersection touches only
    // a boundary. It occupies nothing, so it is not an occupied cell.
    let mut occupied: Vec<f64> = extents
        .iter()
        .copied()
        .filter(|e| *e > 0.0 && e.is_finite())
        .collect();

    // total_cmp rather than partial_cmp: a total order, and no unwrap on NaN.
    occupied.sort_unstable_by(f64::total_cmp);

    let mut total = Neumaier::default();
    for extent in &occupied {
        total.add(*extent);
    }
    let total_extent = total.value();

    if occupied.is_empty() || total_extent <= 0.0 {
        return OnePercentResult {
            aoo_cells: 0,
            occupied_cell_count: 0,
            total_extent: 0.0,
            threshold_margin: f64::INFINITY,
            near_boundary: false,
        };
    }

    let mut cumulative = Neumaier::default();
    let mut kept = 0u32;
    let mut margin = f64::INFINITY;

    for extent in &occupied {
        cumulative.add(*extent);
        let proportion = cumulative.value() / total_extent;
        margin = margin.min((proportion - CUTOFF).abs());
        if proportion > CUTOFF {
            kept += 1;
        }
    }

    OnePercentResult {
        aoo_cells: kept,
        occupied_cell_count: u32::try_from(occupied.len()).unwrap_or(u32::MAX),
        total_extent,
        threshold_margin: margin,
        near_boundary: margin < NEAR_BOUNDARY_MARGIN,
    }
}

/// One ecosystem's extent within one grid cell.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct GridCell {
    /// The ecosystem occupying this cell.
    pub ecosystem: String,
    /// Which cell.
    pub cell: CellId,
    /// Extent of the ecosystem inside the cell, in m².
    pub extent_m2: f64,
    /// Extent as a proportion of the cell, in `0.0..=1.0`.
    pub fraction: f64,
}

/// Per-cell extents, ready for the 1% exclusion.
///
/// The layout is long/tidy — one row per occupied `(ecosystem, cell)` pair — rather
/// than a wide table with a column per ecosystem. For Colombia's 161 ecosystems, a
/// wide table is mostly zeros, and the column names have to be derived from
/// ecosystem codes by a slug rule that can collide (`T1.1` and `T1_1` both become
/// `T1_1`, silently merging two ecosystems).
#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct AooGrid {
    cells: Vec<GridCell>,
    ecosystems: Vec<String>,
    overfull_cells: u32,
}

impl AooGrid {
    /// Every occupied `(ecosystem, cell)` pair, in a deterministic order.
    #[must_use]
    pub fn cells(&self) -> &[GridCell] {
        &self.cells
    }

    /// The ecosystems present, sorted.
    #[must_use]
    pub fn ecosystems(&self) -> &[String] {
        &self.ecosystems
    }

    /// How many cells received more extent than they can hold.
    ///
    /// Non-zero means the source map has overlapping features of the same
    /// ecosystem. The extent is capped at the cell area, but the overlap is a data
    /// problem worth surfacing rather than silently absorbing.
    #[must_use]
    pub const fn overfull_cells(&self) -> u32 {
        self.overfull_cells
    }

    /// The Criterion B2 metric for one ecosystem, after the 1% exclusion.
    #[must_use]
    pub fn aoo(&self, ecosystem: &str) -> OnePercentResult {
        let mut extents: Vec<f64> = self
            .cells
            .iter()
            .filter(|c| c.ecosystem == ecosystem)
            .map(|c| c.extent_m2)
            .collect();
        one_percent_rule(&mut extents)
    }
}

/// Accumulates per-cell extent as features stream past.
///
/// Memory is proportional to the number of *occupied* cells, not to the size of the
/// source map: features are consumed one at a time and never retained, and no grid
/// is materialised — candidate cells come from arithmetic on each feature's bounds.
///
/// ```
/// use iucn_rle_core::aoo::AooAccumulator;
///
/// let mut acc = AooAccumulator::new();
/// // A 1 km square, in metres, inside a single 10 km cell.
/// acc.add_polygon("forest", &[vec![
///     [0.0, 0.0], [1_000.0, 0.0], [1_000.0, 1_000.0], [0.0, 1_000.0],
/// ]]);
///
/// let grid = acc.finish();
/// assert_eq!(grid.aoo("forest").occupied_cell_count, 1);
/// ```
#[derive(Default, Debug)]
pub struct AooAccumulator {
    /// Keyed by interned ecosystem id and cell, so memory tracks occupancy.
    cells: HashMap<(u32, CellId), Neumaier>,
    names: Vec<String>,
    ids: HashMap<String, u32>,
}

impl AooAccumulator {
    /// An accumulator with no features yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern an ecosystem name so cells key on a small integer.
    fn id_for(&mut self, ecosystem: &str) -> u32 {
        if let Some(id) = self.ids.get(ecosystem) {
            return *id;
        }
        let id = u32::try_from(self.names.len()).expect("ecosystem count fits in u32");
        self.names.push(ecosystem.to_owned());
        self.ids.insert(ecosystem.to_owned(), id);
        id
    }

    /// Add one polygon, in projected metres.
    ///
    /// `rings` is the exterior ring followed by any holes. Holes must wind opposite
    /// to the exterior; their opposite signed area then subtracts itself, with no
    /// special-case handling.
    ///
    /// **Either winding convention is accepted.** `GeoJSON` specifies counter-clockwise
    /// exteriors ([RFC 7946 §3.1.6]) while shapefiles use clockwise ones, and plenty
    /// of real data follows neither. The exterior's own orientation sets the sign, so
    /// the source format cannot change an assessment.
    ///
    /// Only cells the polygon actually intersects are touched — its bounding box
    /// selects candidates, and clipping decides which of those really contribute.
    ///
    /// [RFC 7946 §3.1.6]: https://datatracker.ietf.org/doc/html/rfc7946#section-3.1.6
    pub fn add_polygon(&mut self, ecosystem: &str, rings: &[Vec<[f64; 2]>]) {
        let Some(bounds) = bounding_box(rings) else {
            return;
        };
        let Some(exterior) = rings.first() else {
            return;
        };

        // Normalise to a positive exterior. A clockwise exterior would otherwise make
        // every contribution negative and be filtered away as unoccupied — silently
        // yielding an empty result for an entire shapefile.
        let orientation = if ring_area(exterior) < 0.0 { -1.0 } else { 1.0 };

        let id = self.id_for(ecosystem);

        for cell in CellId::covering(bounds) {
            let rect = cell.bounds();
            let mut area = 0.0;
            for ring in rings {
                area += clipped_area(ring, rect);
            }
            area *= orientation;

            // A cell can net to zero (fully inside a hole) or to a whisker of
            // negative float noise. Neither is occupancy.
            if area > 0.0 {
                self.cells.entry((id, cell)).or_default().add(area);
            }
        }
    }

    /// Finish accumulating and produce the per-cell table.
    #[must_use]
    pub fn finish(self) -> AooGrid {
        let mut overfull_cells = 0u32;

        let mut cells: Vec<GridCell> = self
            .cells
            .into_iter()
            .filter_map(|((id, cell), extent)| {
                let raw = extent.value();
                if raw <= 0.0 {
                    return None;
                }
                // A cell cannot be more than fully occupied. Overlapping features of
                // one ecosystem are a data problem; cap the extent so downstream
                // fractions stay meaningful, and count it so the caller is told.
                let extent_m2 = if raw > CELL_AREA_M2 {
                    overfull_cells += 1;
                    CELL_AREA_M2
                } else {
                    raw
                };
                Some(GridCell {
                    ecosystem: self.names[id as usize].clone(),
                    cell,
                    extent_m2,
                    fraction: extent_m2 / CELL_AREA_M2,
                })
            })
            .collect();

        // A HashMap has no order, but assessments get committed and diffed, so the
        // output must be stable across runs.
        cells.sort_unstable_by(|a, b| {
            a.ecosystem
                .cmp(&b.ecosystem)
                .then(a.cell.col.cmp(&b.cell.col))
                .then(a.cell.row.cmp(&b.cell.row))
        });

        let mut ecosystems = self.names;
        ecosystems.sort_unstable();

        AooGrid {
            cells,
            ecosystems,
            overfull_cells,
        }
    }
}

/// Bounding box of a set of rings, or `None` when there is nothing to bound.
fn bounding_box(rings: &[Vec<[f64; 2]>]) -> Option<[f64; 4]> {
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut any = false;

    for ring in rings {
        for &[x, y] in ring {
            if x.is_finite() && y.is_finite() {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                any = true;
            }
        }
    }

    any.then_some([min_x, min_y, max_x, max_y])
}
