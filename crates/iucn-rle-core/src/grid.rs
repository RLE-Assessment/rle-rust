//! The global AOO grid.
//!
//! The grid is **global and fixed**, not derived from the data being assessed. Cells
//! are indexed from (0, 0) at the projection origin, so the same ground position
//! falls in the same cell in every assessment ever made. That is what makes AOO
//! counts comparable between ecosystems and between assessors — a grid fitted to
//! each dataset's own extent would not be.

use serde::{Deserialize, Serialize};

/// Cell size, fixed by the Guidelines at 10 × 10 km (§6.3.2, p. 67).
///
/// Not a tunable parameter. The Guidelines give four reasons for this grain,
/// including that ecosystem boundaries are inherently vague and that larger cells
/// predict real-world risk better than finer ones.
pub const AOO_CELL_SIZE_M: f64 = 10_000.0;

/// The coordinate reference system the grid is defined in.
///
/// World Cylindrical Equal Area. Equal-area is essential: cells must represent the
/// same ground area everywhere, or a count of them means nothing.
pub const AOO_CRS: &str = "ESRI:54034";

/// A cell in the global AOO grid, addressed by column and row.
///
/// Ordered by column then row, so collections of cells sort deterministically —
/// output that gets committed to a repository and diffed must not depend on hash
/// iteration order.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct CellId {
    /// Column index; `0` is the cell whose left edge is the projection origin.
    pub col: i32,
    /// Row index; `0` is the cell whose bottom edge is the projection origin.
    pub row: i32,
}

impl CellId {
    /// A cell at the given column and row.
    #[must_use]
    pub const fn new(col: i32, row: i32) -> Self {
        Self { col, row }
    }

    /// The cell containing a projected coordinate, in metres.
    ///
    /// Cells are half-open: a cell owns its lower-left corner and not its
    /// upper-right, so a point on a shared edge belongs to exactly one cell.
    ///
    /// Uses `floor`, **not** a cast. Casting truncates toward zero, which would put
    /// -5,000 and +5,000 in the same cell and silently merge the western and eastern
    /// hemispheres — and the southern and northern ones.
    ///
    /// ```
    /// use iucn_rle_core::grid::CellId;
    ///
    /// assert_eq!(CellId::containing(-5_000.0, -5_000.0), CellId::new(-1, -1));
    /// ```
    #[must_use]
    pub fn containing(x_m: f64, y_m: f64) -> Self {
        Self {
            col: floor_div(x_m),
            row: floor_div(y_m),
        }
    }

    /// The cell's bounds as `[min_x, min_y, max_x, max_y]`, in metres.
    #[must_use]
    pub fn bounds(self) -> [f64; 4] {
        let min_x = f64::from(self.col) * AOO_CELL_SIZE_M;
        let min_y = f64::from(self.row) * AOO_CELL_SIZE_M;
        [
            min_x,
            min_y,
            min_x + AOO_CELL_SIZE_M,
            min_y + AOO_CELL_SIZE_M,
        ]
    }

    /// Every cell a bounding box touches, as `[min_x, min_y, max_x, max_y]`.
    ///
    /// This is how a feature's candidate cells are found: arithmetic on its bounds,
    /// with no grid materialised and no spatial join. Memory stays proportional to
    /// the cells a feature actually touches rather than to the extent of the map.
    ///
    /// An inverted or empty box yields nothing.
    pub fn covering(bounds: [f64; 4]) -> impl Iterator<Item = Self> {
        let [min_x, min_y, max_x, max_y] = bounds;

        // NaN fails every comparison, so this rejects it as well as inverted boxes.
        let usable = min_x <= max_x
            && min_y <= max_y
            && min_x.is_finite()
            && min_y.is_finite()
            && max_x.is_finite()
            && max_y.is_finite();

        let (first, last) = if usable {
            (
                Self::containing(min_x, min_y),
                Self::containing(max_x, max_y),
            )
        } else {
            // An impossible range, so the iterator is empty.
            (Self::new(0, 0), Self::new(-1, -1))
        };

        (first.row..=last.row)
            .flat_map(move |row| (first.col..=last.col).map(move |col| Self::new(col, row)))
    }
}

/// Floor division of a coordinate by the cell size.
///
/// Separated out because getting this wrong is the single easiest way to corrupt
/// every assessment in the southern or western hemisphere.
// The cast below is guarded by the range checks immediately above it, and NaN falls
// through to 0 rather than to an arbitrary cell.
#[allow(clippy::cast_possible_truncation)]
fn floor_div(coordinate_m: f64) -> i32 {
    let index = (coordinate_m / AOO_CELL_SIZE_M).floor();
    // Saturating rather than wrapping: a coordinate this far out is nonsense, and
    // wrapping would place it in a plausible-looking cell on the other side of the
    // world rather than an obviously wrong one.
    if index >= f64::from(i32::MAX) {
        i32::MAX
    } else if index <= f64::from(i32::MIN) {
        i32::MIN
    } else if index.is_nan() {
        0
    } else {
        index as i32
    }
}
