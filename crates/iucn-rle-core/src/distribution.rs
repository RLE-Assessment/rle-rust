//! From a distribution map to Criterion B metrics.
//!
//! This is the path an assessment takes: polygons in longitude and latitude go in,
//! and an extent of occurrence and area of occupancy come out. Everything below this
//! layer works in projected metres.
//!
//! Both metrics are accumulated **incrementally**, so a national map with hundreds of
//! thousands of features never needs to be held in memory:
//!
//! * AOO retains only occupied cells, so memory tracks the ecosystem's footprint.
//! * EOO retains only the running convex hull, which stays small because the hull of
//!   a union is the hull of the combined vertices — so folding each new feature into
//!   the existing hull loses nothing.
//!
//! # A deliberate difference from `rle-python`
//!
//! The hull is computed in the **projected plane**, not in longitude/latitude and
//! then reprojected. Convexity is a planar property, and the Guidelines define the
//! EOO as a polygon "in which no internal angle exceeds 180°" whose area is then
//! measured — so the polygon and its area belong in the same plane. `rle-python`
//! takes the hull in EPSG:4326 degrees and reprojects the result, which is a
//! different polygon: a straight line between two points in degrees is not straight
//! in an equal-area projection. The difference is negligible for small extents and
//! grows with the span of the distribution.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::aoo::{AooAccumulator, AooGrid, OnePercentResult};
use crate::eoo::convex_hull;
use crate::geometry::ring_area;
use crate::projection::project_to_aoo_crs;

/// Square metres per square kilometre.
const M2_PER_KM2: f64 = 1_000_000.0;

/// Criterion B metrics for a set of ecosystems.
#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct Distribution {
    grid: AooGrid,
    /// Final convex hull per ecosystem, in projected metres.
    hulls: HashMap<String, Vec<[f64; 2]>>,
}

impl Distribution {
    /// The ecosystems present, sorted.
    #[must_use]
    pub fn ecosystems(&self) -> &[String] {
        self.grid.ecosystems()
    }

    /// The per-cell extent table underlying the AOO.
    #[must_use]
    pub const fn grid(&self) -> &AooGrid {
        &self.grid
    }

    /// Extent of occurrence in km² — the Criterion B1 metric.
    ///
    /// Zero when the ecosystem has fewer than three distinct occurrences, or when
    /// they are collinear. Deliberately not a small positive number: a fabricated
    /// area here can turn a single mapped occurrence into a Critically Endangered
    /// listing.
    #[must_use]
    pub fn eoo_km2(&self, ecosystem: &str) -> f64 {
        self.hulls.get(ecosystem).map_or(0.0, |hull| {
            if hull.len() < 3 {
                0.0
            } else {
                ring_area(hull).abs() / M2_PER_KM2
            }
        })
    }

    /// Area of occupancy — the Criterion B2 metric, after the 1% exclusion.
    #[must_use]
    pub fn aoo(&self, ecosystem: &str) -> OnePercentResult {
        self.grid.aoo(ecosystem)
    }

    /// Number of vertices in an ecosystem's convex hull.
    ///
    /// Exposed mainly so the streaming property is testable: this stays small however
    /// many features were consumed.
    #[must_use]
    pub fn hull_vertices(&self, ecosystem: &str) -> usize {
        self.hulls.get(ecosystem).map_or(0, Vec::len)
    }

    /// An ecosystem's convex hull, in projected metres.
    #[must_use]
    pub fn hull(&self, ecosystem: &str) -> &[[f64; 2]] {
        self.hulls.get(ecosystem).map_or(&[], Vec::as_slice)
    }
}

/// Builds [`Distribution`] from geographic polygons, one feature at a time.
///
/// ```
/// use iucn_rle_core::distribution::DistributionAccumulator;
///
/// let mut acc = DistributionAccumulator::new();
/// acc.add_polygon("forest", &[vec![
///     [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
/// ]]);
///
/// let distribution = acc.finish();
/// assert!(distribution.eoo_km2("forest") > 12_000.0);   // roughly 111 km square
/// assert!(distribution.aoo("forest").aoo_cells > 100);  // in 10 km cells
/// ```
#[derive(Default, Debug)]
pub struct DistributionAccumulator {
    aoo: AooAccumulator,
    /// Running convex hull per ecosystem, in projected metres. Folding each feature
    /// into the hull immediately is what keeps memory bounded.
    hulls: HashMap<String, Vec<[f64; 2]>>,
}

impl DistributionAccumulator {
    /// An accumulator with no features yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one polygon in longitude/latitude degrees on WGS84.
    ///
    /// `rings` is the exterior ring followed by any holes. Either winding convention
    /// is accepted — see [`AooAccumulator::add_polygon`].
    ///
    /// Holes contribute to the AOO but **not** to the EOO hull: an occurrence is
    /// still an occurrence at the outer boundary, and the hull must enclose all of
    /// them regardless of what is missing inside.
    pub fn add_polygon(&mut self, ecosystem: &str, rings: &[Vec<[f64; 2]>]) {
        if rings.is_empty() {
            return;
        }

        // Project once; both metrics consume the projected geometry.
        let projected: Vec<Vec<[f64; 2]>> = rings
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|&[lon, lat]| {
                        let (x, y) = project_to_aoo_crs(lon, lat);
                        [x, y]
                    })
                    .collect()
            })
            .collect();

        self.aoo.add_polygon(ecosystem, &projected);

        // Fold the exterior's vertices into the running hull. Only the exterior
        // matters: a hole describes absence inside the distribution, not an
        // occurrence, and cannot extend the outer boundary.
        let hull = self.hulls.entry(ecosystem.to_owned()).or_default();
        hull.extend_from_slice(&projected[0]);
        *hull = convex_hull(hull);
    }

    /// Finish and produce the metrics.
    #[must_use]
    pub fn finish(self) -> Distribution {
        Distribution {
            grid: self.aoo.finish(),
            hulls: self.hulls,
        }
    }
}
