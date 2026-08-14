//! Area of occupancy: the Criterion B2 metric.
//!
//! AOO is a **count of occupied 10 × 10 km grid cells**, not an area. The grid size
//! is fixed by the Guidelines rather than chosen per assessment, because extent
//! estimates are extremely sensitive to the resolution of the source map and a
//! standard grain is what makes assessments comparable (§6.3.2, p. 67).

use serde::{Deserialize, Serialize};

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
