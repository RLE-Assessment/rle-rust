//! Extent of occurrence: the Criterion B1 metric.
//!
//! The EOO is the area of the minimum convex polygon enclosing every known
//! occurrence — "the smallest polygon in which no internal angle exceeds 180°"
//! (§6.3.2, p. 67).
//!
//! # The rule that is most often broken
//!
//! The hull **must not exclude anything**:
//!
//! > The minimum convex polygon (also known as a convex hull) must not exclude any
//! > areas, discontinuities or disjunctions, regardless of whether the ecosystem can
//! > occur in those areas or not. Regions such as oceans (for terrestrial
//! > ecosystems), land (for coastal or marine ecosystems), or areas outside the study
//! > area (such as in a different country) must remain included within the minimum
//! > convex polygon.
//!
//! Clipping the hull to where the ecosystem actually occurs produces a smaller EOO,
//! and therefore an inflated threat category, and makes the number incomparable with
//! every other assessment. This module cannot enforce that — it depends on which
//! points a caller supplies — but the constraint belongs next to the computation.
//!
//! EOO measures how spread out the risk is, not how much ecosystem there is.

use crate::geometry::ring_area;

/// Square metres per square kilometre.
const M2_PER_KM2: f64 = 1_000_000.0;

/// The convex hull of a point set, counter-clockwise, without collinear vertices.
///
/// Andrew's monotone chain: sort, then build the lower and upper hulls. `O(n log n)`,
/// and it needs no polygon union first — the convex hull of a union of shapes is the
/// convex hull of their combined vertices, so the expensive and failure-prone union
/// step that `rle-python` performs is unnecessary.
///
/// Returns fewer than three points when the input is degenerate: fewer than three
/// distinct points, or all of them collinear.
///
/// ```
/// use iucn_rle_core::eoo::convex_hull;
///
/// let points = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [5.0, 5.0]];
/// assert_eq!(convex_hull(&points).len(), 4); // the interior point is not a vertex
/// ```
#[must_use]
pub fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut sorted: Vec<[f64; 2]> = points
        .iter()
        .copied()
        .filter(|[x, y]| x.is_finite() && y.is_finite())
        .collect();

    sorted.sort_unstable_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    sorted.dedup();

    if sorted.len() < 3 {
        return sorted;
    }

    // Strictly-positive cross product only, so vertices at exactly 180° are dropped
    // and the polygon really is minimum.
    let mut hull: Vec<[f64; 2]> = Vec::with_capacity(sorted.len() * 2);

    for &point in &sorted {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0 {
            hull.pop();
        }
        hull.push(point);
    }

    let lower_len = hull.len() + 1;
    for &point in sorted.iter().rev().skip(1) {
        while hull.len() >= lower_len
            && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0
        {
            hull.pop();
        }
        hull.push(point);
    }

    // The last point repeats the first.
    hull.pop();

    // All input collinear: the chains collapse to a segment enclosing no area.
    if hull.len() < 3 {
        return Vec::new();
    }
    hull
}

/// Cross product of `oa` and `ob`; positive when `o`→`a`→`b` turns counter-clockwise.
fn cross(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]).mul_add(b[1] - o[1], -((a[1] - o[1]) * (b[0] - o[0])))
}

/// Extent of occurrence in km², from occurrence coordinates in metres.
///
/// Coordinates must already be in an equal-area projected CRS; this is planar
/// arithmetic and is not valid on a sphere.
///
/// Degenerate inputs — fewer than three distinct points, or wholly collinear ones —
/// give exactly `0.0`. That matters: `rle-python` buffers a degenerate hull by
/// 0.0001 so its map renderer has something to draw, and that fabricated area then
/// flows into the B1 threshold, where it can manufacture a Critically Endangered
/// listing out of a single mapped occurrence.
///
/// ```
/// use iucn_rle_core::eoo::eoo_km2;
///
/// // A 100 km square, in metres.
/// let points = [[0.0, 0.0], [100_000.0, 0.0], [100_000.0, 100_000.0], [0.0, 100_000.0]];
/// assert!((eoo_km2(&points) - 10_000.0).abs() < 1e-9);
/// ```
#[must_use]
pub fn eoo_km2(points: &[[f64; 2]]) -> f64 {
    let hull = convex_hull(points);
    if hull.len() < 3 {
        return 0.0;
    }
    ring_area(&hull).abs() / M2_PER_KM2
}
