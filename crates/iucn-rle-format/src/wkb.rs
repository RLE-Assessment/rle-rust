//! Well-Known Binary geometry decoding.
//!
//! WKB is how `GeoParquet` stores geometry, so every remote vector read arrives here.
//! The format is self-describing and little of it is optional, which makes a hand-written
//! decoder both short and safe — and avoids a dependency that would pull in a whole
//! geometry stack for the sake of one function.
//!
//! Three dialects appear in the wild and all three are accepted:
//!
//! * **OGC WKB** — type codes 1–7, two-dimensional.
//! * **ISO WKB** — adds 1000 for Z, 2000 for M, 3000 for both.
//! * **`PostGIS` EWKB** — sets high bits instead, and may carry an SRID after the type.
//!
//! Extra ordinates are read and discarded rather than rejected. An assessment is a
//! planar calculation, so Z and M carry no information for it, but *skipping* them is
//! the load-bearing part: treating a `PolygonZ` as two-dimensional would shift every
//! coordinate after the first and yield a plausible, silently wrong shape.

/// A linear ring: a closed sequence of `[x, y]` coordinates.
pub type Ring = Vec<[f64; 2]>;

/// A polygon as its rings, exterior first, holes after.
///
/// Ring *order* carries the meaning — position decides which ring is the exterior, not
/// winding direction. Real data winds inconsistently, so anything that reorders rings
/// turns holes into patches.
pub type Polygon = Vec<Ring>;

/// Something that went wrong reading WKB.
#[derive(Debug, thiserror::Error)]
pub enum WkbError {
    /// The byte-order marker was neither 0 (big-endian) nor 1 (little-endian).
    #[error("unknown WKB byte-order marker {0}, expected 0 or 1")]
    BadByteOrder(u8),

    /// The bytes ended sooner than the structure said they would.
    ///
    /// Always an error, never a partial result: half a coordinate list would otherwise
    /// decode into a smaller, valid-looking polygon and understate the metrics.
    #[error("WKB ended while reading {what}, with {available} bytes left")]
    Truncated {
        /// What was being read when the bytes ran out.
        what: &'static str,
        /// Bytes that remained.
        available: usize,
    },

    /// The geometry is well-formed but has no area.
    ///
    /// A point or line in an ecosystem distribution is a data problem. Returning no
    /// polygons instead would understate the AOO with nothing to explain why.
    #[error("expected an areal geometry, found {name}")]
    NotAreal {
        /// Human-readable name of the geometry type found.
        name: String,
    },
}

/// EWKB flag bits, which `PostGIS` sets instead of adding thousands to the type code.
const EWKB_Z: u32 = 0x8000_0000;
const EWKB_M: u32 = 0x4000_0000;
const EWKB_SRID: u32 = 0x2000_0000;
const EWKB_FLAGS: u32 = EWKB_Z | EWKB_M | EWKB_SRID;

const TYPE_POLYGON: u32 = 3;
const TYPE_MULTIPOLYGON: u32 = 6;

/// Decode a WKB geometry into polygons.
///
/// A `Polygon` yields one; a `MultiPolygon` yields one per part, kept separate because
/// each part contributes independently to both EOO and AOO.
///
/// # Errors
///
/// See [`WkbError`]. Every structural problem is reported rather than worked around.
pub fn decode_polygons(bytes: &[u8]) -> Result<Vec<Polygon>, WkbError> {
    let mut cursor = Cursor::new(bytes);
    let header = cursor.geometry_header()?;

    match header.kind {
        TYPE_POLYGON => Ok(vec![cursor.polygon_body(&header)?]),
        TYPE_MULTIPOLYGON => {
            let count =
                cursor.count(header.little, "multipolygon part count", MIN_POLYGON_BYTES)?;
            let mut polygons = Vec::with_capacity(count);
            for _ in 0..count {
                // Each part carries its own byte-order marker and type code; assuming
                // the container's applies throughout is a classic WKB bug.
                let part = cursor.geometry_header()?;
                if part.kind != TYPE_POLYGON {
                    return Err(WkbError::NotAreal {
                        name: format!("{} inside a MultiPolygon", type_name(part.kind)),
                    });
                }
                polygons.push(cursor.polygon_body(&part)?);
            }
            Ok(polygons)
        }
        other => Err(WkbError::NotAreal {
            name: type_name(other).to_owned(),
        }),
    }
}

/// Smallest possible encoding of a nested polygon: marker, type, and a ring count.
const MIN_POLYGON_BYTES: usize = 1 + 4 + 4;
/// Smallest possible encoding of a ring: a point count.
const MIN_RING_BYTES: usize = 4;

/// What a geometry's header established, applying to that geometry only.
struct Header {
    /// Base type code with dialect flags stripped: 1 = Point, 3 = Polygon, and so on.
    kind: u32,
    /// True if the geometry is little-endian.
    little: bool,
    /// Ordinates per coordinate: 2, 3 for Z or M, 4 for both.
    dimensions: usize,
}

fn type_name(kind: u32) -> &'static str {
    match kind {
        1 => "Point",
        2 => "LineString",
        3 => "Polygon",
        4 => "MultiPoint",
        5 => "MultiLineString",
        6 => "MultiPolygon",
        7 => "GeometryCollection",
        _ => "an unknown geometry type",
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, length: usize, what: &'static str) -> Result<&'a [u8], WkbError> {
        let end = self
            .position
            .checked_add(length)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(WkbError::Truncated {
                what,
                available: self.remaining(),
            })?;
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn u32(&mut self, little: bool, what: &'static str) -> Result<u32, WkbError> {
        let raw: [u8; 4] = self.take(4, what)?.try_into().expect("took four bytes");
        Ok(if little {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        })
    }

    fn f64(&mut self, little: bool, what: &'static str) -> Result<f64, WkbError> {
        let raw: [u8; 8] = self.take(8, what)?.try_into().expect("took eight bytes");
        Ok(if little {
            f64::from_le_bytes(raw)
        } else {
            f64::from_be_bytes(raw)
        })
    }

    /// Read a byte-order marker, type code, and — for EWKB — an SRID.
    fn geometry_header(&mut self) -> Result<Header, WkbError> {
        let little = match self.take(1, "byte-order marker")?[0] {
            0 => false,
            1 => true,
            other => return Err(WkbError::BadByteOrder(other)),
        };

        let code = self.u32(little, "geometry type")?;

        if code & EWKB_SRID != 0 {
            // Present but irrelevant: the CRS comes from the file's metadata, which is
            // authoritative for the whole column. Read past it.
            self.u32(little, "EWKB SRID")?;
        }

        // The two dialects are distinguishable: EWKB sets high bits, ISO adds thousands.
        // A code can only be one or the other, so checking both is safe.
        let iso = code & !EWKB_FLAGS;
        let has_z = code & EWKB_Z != 0 || iso / 1000 == 1 || iso / 1000 == 3;
        let has_m = code & EWKB_M != 0 || iso / 1000 == 2 || iso / 1000 == 3;

        Ok(Header {
            kind: iso % 1000,
            little,
            dimensions: 2 + usize::from(has_z) + usize::from(has_m),
        })
    }

    /// Read a count, rejecting it immediately if the bytes it implies are not present.
    ///
    /// Checking before allocating is the point. A corrupt length field claiming four
    /// billion rings must fail on the bytes not being there, not on attempting the
    /// allocation first.
    fn count(
        &mut self,
        little: bool,
        what: &'static str,
        min_bytes_each: usize,
    ) -> Result<usize, WkbError> {
        let declared =
            usize::try_from(self.u32(little, what)?).map_err(|_| WkbError::Truncated {
                what,
                available: self.remaining(),
            })?;

        let needed = declared
            .checked_mul(min_bytes_each)
            .ok_or(WkbError::Truncated {
                what,
                available: self.remaining(),
            })?;
        if needed > self.remaining() {
            return Err(WkbError::Truncated {
                what,
                available: self.remaining(),
            });
        }
        Ok(declared)
    }

    /// Read a polygon's rings, its header already consumed.
    fn polygon_body(&mut self, header: &Header) -> Result<Polygon, WkbError> {
        let ring_count = self.count(header.little, "ring count", MIN_RING_BYTES)?;
        let mut rings = Vec::with_capacity(ring_count);

        for _ in 0..ring_count {
            let point_count = self.count(header.little, "point count", header.dimensions * 8)?;
            let mut ring = Vec::with_capacity(point_count);

            for _ in 0..point_count {
                let x = self.f64(header.little, "x coordinate")?;
                let y = self.f64(header.little, "y coordinate")?;
                for _ in 2..header.dimensions {
                    // Z and M are read and dropped. Not skipping them would shift every
                    // later coordinate and produce a wrong shape that still looks valid.
                    self.f64(header.little, "extra ordinate")?;
                }
                ring.push([x, y]);
            }
            rings.push(ring);
        }
        Ok(rings)
    }
}
