//! Polygon clipping and area, with no geometry-engine dependency.
//!
//! The AOO computation needs exactly one geometric operation: how much of a polygon
//! falls inside an axis-aligned rectangle. A grid cell is convex, so
//! Sutherland-Hodgman clipping is exact and closed-form for this case — no noding,
//! no snapping, no robustness predicates, and no failure mode. A general
//! boolean-overlay engine would be slower, larger, and would drag in a C++
//! dependency that cannot target WebAssembly.
//!
//! Coordinates are `[x, y]` pairs in a projected, equal-area CRS. Everything here is
//! planar arithmetic; nothing is valid on a sphere.

/// A polygon ring: an ordered sequence of vertices.
///
/// The closing vertex may be repeated or omitted; both give the same answer, because
/// source formats disagree and the format must not change an assessment.
pub type Ring<'a> = &'a [[f64; 2]];

/// Signed area of a ring, by the shoelace formula.
///
/// Counter-clockwise winding is positive, clockwise negative. The sign is what makes
/// holes work: sum an exterior ring with its holes and the holes subtract themselves,
/// with no special-case bookkeeping.
///
/// ```
/// use iucn_rle_core::geometry::ring_area;
///
/// let square = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
/// assert!((ring_area(&square) - 4.0).abs() < 1e-12);
/// ```
#[must_use]
pub fn ring_area(ring: Ring<'_>) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }

    // Doubled area accumulated first, halved once at the end: one rounding step
    // instead of one per edge.
    let mut doubled = 0.0;
    for i in 0..ring.len() {
        let [x0, y0] = ring[i];
        let [x1, y1] = ring[(i + 1) % ring.len()];
        doubled += x0.mul_add(y1, -(x1 * y0));
    }
    doubled / 2.0
}

/// Which side of a clip edge a point is on.
///
/// The rectangle is clipped edge by edge; each edge is a half-plane test.
#[derive(Copy, Clone)]
enum Edge {
    Left(f64),
    Right(f64),
    Bottom(f64),
    Top(f64),
}

impl Edge {
    /// Whether a point is on the kept side of this edge, boundary included.
    fn contains(self, [x, y]: [f64; 2]) -> bool {
        match self {
            Self::Left(min_x) => x >= min_x,
            Self::Right(max_x) => x <= max_x,
            Self::Bottom(min_y) => y >= min_y,
            Self::Top(max_y) => y <= max_y,
        }
    }

    /// Where the segment `a`-`b` crosses this edge.
    ///
    /// Only called when the endpoints are on opposite sides, so the denominator is
    /// non-zero and the interpolation parameter lies in `[0, 1]`.
    fn intersect(self, a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
        let [ax, ay] = a;
        let [bx, by] = b;
        match self {
            Self::Left(v) | Self::Right(v) => {
                let t = (v - ax) / (bx - ax);
                [v, ay + t * (by - ay)]
            }
            Self::Bottom(v) | Self::Top(v) => {
                let t = (v - ay) / (by - ay);
                [ax + t * (bx - ax), v]
            }
        }
    }
}

/// Clip a ring against one half-plane, writing the result into `output`.
fn clip_to_edge(input: &[[f64; 2]], edge: Edge, output: &mut Vec<[f64; 2]>) {
    output.clear();
    if input.is_empty() {
        return;
    }

    let mut previous = input[input.len() - 1];
    let mut previous_inside = edge.contains(previous);

    for &current in input {
        let current_inside = edge.contains(current);

        if current_inside != previous_inside {
            output.push(edge.intersect(previous, current));
        }
        if current_inside {
            output.push(current);
        }

        previous = current;
        previous_inside = current_inside;
    }
}

/// Signed area of the part of `ring` lying inside `cell`.
///
/// `cell` is `[min_x, min_y, max_x, max_y]`. The sign follows the ring's winding,
/// exactly as [`ring_area`] does, so holes compose by summation.
///
/// Summing this over every cell a feature touches recovers the feature's total area,
/// which is the invariant the per-cell fractions and the 1% exclusion depend on.
///
/// ```
/// use iucn_rle_core::geometry::clipped_area;
///
/// // A 2x2 square straddling the cell's left edge: half of it is inside.
/// let ring = [[-1.0, 0.0], [1.0, 0.0], [1.0, 2.0], [-1.0, 2.0]];
/// let cell = [0.0, 0.0, 10.0, 10.0];
/// assert!((clipped_area(&ring, cell) - 2.0).abs() < 1e-12);
/// ```
#[must_use]
pub fn clipped_area(ring: Ring<'_>, cell: [f64; 4]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let [min_x, min_y, max_x, max_y] = cell;

    // Reject by bounding box before doing any clipping work. Most cells a feature's
    // bounds span are not actually touched by it, so this is the common path.
    let (mut rx0, mut ry0) = (f64::INFINITY, f64::INFINITY);
    let (mut rx1, mut ry1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &[x, y] in ring {
        rx0 = rx0.min(x);
        ry0 = ry0.min(y);
        rx1 = rx1.max(x);
        ry1 = ry1.max(y);
    }
    if rx1 <= min_x || rx0 >= max_x || ry1 <= min_y || ry0 >= max_y {
        return 0.0;
    }

    let mut buffer_a: Vec<[f64; 2]> = ring.to_vec();
    let mut buffer_b: Vec<[f64; 2]> = Vec::with_capacity(ring.len() + 4);

    for edge in [
        Edge::Left(min_x),
        Edge::Right(max_x),
        Edge::Bottom(min_y),
        Edge::Top(max_y),
    ] {
        clip_to_edge(&buffer_a, edge, &mut buffer_b);
        core::mem::swap(&mut buffer_a, &mut buffer_b);
        if buffer_a.is_empty() {
            return 0.0;
        }
    }

    ring_area(&buffer_a)
}
