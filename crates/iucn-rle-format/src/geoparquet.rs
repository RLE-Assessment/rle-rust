//! Reading `GeoParquet` without downloading the file.
//!
//! Parquet is laid out so a client can read the footer, decide which row groups it
//! wants, and fetch only those. That is what makes a national dataset readable from a
//! browser tab, and it is why this crate exists rather than deferring to a file reader.
//!
//! Everything here is synchronous and takes bytes that are already in memory. Deciding
//! *which* bytes is the caller's job — see [`footer_range`] and [`Footer::NeedMore`] —
//! and fetching them is `iucn-rle-io`'s. That split is what keeps `Send` bounds out and
//! lets the same code run in a browser.

use core::ops::Range;

use bytes::Bytes;
use std::sync::Arc;

use parquet::file::metadata::{ParquetMetaData, ParquetMetaDataReader, RowGroupMetaData};
use serde::Deserialize;

/// Bytes to fetch on the first request.
///
/// Large enough that one request suffices for the overwhelming majority of files, and
/// small enough to be cheap when it does not. A footer holds per-column statistics for
/// every row group, so it grows with both width and row-group count.
pub const DEFAULT_FOOTER_PREFETCH: u64 = 64 * 1024;

/// Length of the trailer: a four-byte metadata length followed by the magic.
const TRAILER_LEN: usize = 8;
const MAGIC: &[u8; 4] = b"PAR1";
/// Written in place of the magic when the file uses modular encryption.
const MAGIC_ENCRYPTED: &[u8; 4] = b"PARE";

/// Something that went wrong reading a `GeoParquet` file.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The bytes are not a parquet file at all.
    ///
    /// Usually a URL that returned an error page, a redirect body, or HTML. Saying so
    /// beats surfacing a thrift decoding failure from deep inside the parquet reader.
    #[error("not a parquet file: {reason}")]
    NotParquet {
        /// What was wrong with the bytes.
        reason: String,
    },

    /// The file is parquet, but encrypted.
    #[error("this parquet file is encrypted (PARE), which is not supported")]
    Encrypted,

    /// Valid parquet, but carrying no geometry.
    #[error(
        "this parquet file has no `geo` metadata key, so it is plain parquet rather \
         than GeoParquet"
    )]
    NotGeoParquet,

    /// The footer was found but could not be decoded.
    #[error("could not decode the parquet footer: {0}")]
    Footer(String),

    /// The `geo` metadata was present but malformed.
    #[error("the `geo` metadata is not valid GeoParquet: {0}")]
    BadGeoMetadata(String),

    /// A requested column is not in the file.
    ///
    /// The likeliest user error by some margin: national datasets spell the ecosystem
    /// column differently in every country, so the message lists what is available.
    #[error("no column named `{wanted}` in this file; it has: {available}")]
    NoSuchColumn {
        /// The column that was asked for.
        wanted: String,
        /// The columns the file actually has, comma-separated.
        available: String,
    },

    /// A column held a type this reader cannot decode.
    #[error("column `{column}` has type {found}, which cannot be read as {expected}")]
    UnexpectedType {
        /// The column concerned.
        column: String,
        /// The Arrow type found.
        found: String,
        /// What was expected.
        expected: &'static str,
    },

    /// Geometry could not be decoded.
    #[error("could not decode geometry in `{column}`: {source}")]
    Geometry {
        /// The geometry column.
        column: String,
        /// The underlying WKB failure.
        source: crate::wkb::WkbError,
    },

    /// The bytes needed were not supplied.
    #[error(
        "bytes {start}..{end} were needed but not fetched; read the ranges the plan \
         asked for"
    )]
    MissingBytes {
        /// Start of the range needed.
        start: u64,
        /// End of the range needed.
        end: u64,
    },

    /// Reading the row group failed.
    #[error("could not read row group: {0}")]
    Read(String),

    /// The file is compressed with a codec this build cannot decode.
    ///
    /// Raised from the footer rather than left to surface mid-decode, where the
    /// underlying message is a bare "Disabled feature at compile time".
    #[error(
        "this file's geometry is {codec}-compressed, which this build cannot decode: {reason}"
    )]
    UnsupportedCompression {
        /// The codec the file used.
        codec: String,
        /// Why it is unavailable, and what to do instead.
        reason: &'static str,
    },
}

#[cfg(feature = "schema-validation")]
pub use crate::schema::{published_schema_versions, schema_for_version};

/// How serious a structural finding is.
///
/// Ordered so that sorting puts the things worth acting on first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The file cannot be read correctly as it stands.
    Error,
    /// Readable, but something is likely to surprise: a lost optimisation, a
    /// declaration that does not match the data.
    Warning,
    /// Worth knowing, not worth fixing.
    Note,
}

/// Something noticed about a file's metadata.
#[derive(Debug, Clone)]
pub struct Finding {
    /// How serious it is.
    pub severity: Severity,
    /// What was noticed, in terms a user can act on.
    pub message: String,
}

/// Paths to the four bbox covering columns.
///
/// Each is a path rather than a name because the columns live inside a struct, so the
/// leaf is addressed as `bbox.xmin` and not `xmin`.
#[derive(Debug, Clone)]
pub struct Covering {
    /// Path to the minimum-x column.
    pub xmin: Vec<String>,
    /// Path to the minimum-y column.
    pub ymin: Vec<String>,
    /// Path to the maximum-x column.
    pub xmax: Vec<String>,
    /// Path to the maximum-y column.
    pub ymax: Vec<String>,
}

/// One geometry column's entry in the `geo` metadata.
#[derive(Debug, Clone, Deserialize)]
struct GeoColumn {
    encoding: String,
    #[serde(default)]
    crs: Option<serde_json::Value>,
    #[serde(default)]
    covering: Option<RawCovering>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCovering {
    bbox: RawBbox,
}

#[derive(Debug, Clone, Deserialize)]
struct RawBbox {
    xmin: Vec<String>,
    ymin: Vec<String>,
    xmax: Vec<String>,
    ymax: Vec<String>,
}

/// The parsed `geo` metadata key.
#[derive(Debug, Clone, Deserialize)]
pub struct GeoMetadata {
    /// The `GeoParquet` version the file declares.
    ///
    /// Informational only. Notably it must **not** be used to decide which features are
    /// available: geopandas 1.1.4 declares `1.0.0` while writing the `covering` key
    /// introduced in 1.1.0, so a version gate would discard bbox pruning on files from
    /// the most widely used writer there is.
    pub version: String,
    /// Name of the geometry column to read.
    ///
    /// A file may hold several. Assuming the name is `geometry` works for geopandas and
    /// fails on `PostGIS` exports, where `geom` and `wkb_geometry` are common.
    pub primary_column: String,
    columns: std::collections::HashMap<String, GeoColumn>,
    #[serde(skip)]
    covering: Option<Covering>,
    /// The metadata exactly as the writer wrote it.
    ///
    /// Kept verbatim because validation must judge what is in the file, not what this
    /// crate's types happen to round-trip to.
    #[serde(skip)]
    raw: String,
}

impl GeoMetadata {
    fn primary(&self) -> Option<&GeoColumn> {
        self.columns.get(&self.primary_column)
    }

    /// Encoding of the primary geometry column, as the file declares it.
    #[must_use]
    pub fn primary_encoding(&self) -> &str {
        self.primary().map_or("", |column| column.encoding.as_str())
    }

    /// Whether the primary column holds WKB.
    ///
    /// `GeoParquet` 1.1 also allows native Arrow geometry encodings, which this reader
    /// does not yet decode.
    #[must_use]
    pub fn primary_is_wkb(&self) -> bool {
        self.primary_encoding().eq_ignore_ascii_case("WKB")
    }

    /// Authority-qualified CRS code of the primary column, such as `EPSG:4326`.
    ///
    /// The CRS is stored as PROJJSON, which carries a full datum definition; only the
    /// authority code is extracted here, because that is all a reprojection needs to
    /// look up. Returns `None` when the file omits it — which per the specification
    /// means OGC:CRS84, a decision left to the caller rather than silently applied.
    #[must_use]
    pub fn primary_crs_code(&self) -> Option<String> {
        let id = self.primary()?.crs.as_ref()?.get("id")?;
        let authority = id.get("authority")?.as_str()?;
        let code = id.get("code")?;
        let code = code
            .as_u64()
            .map(|number| number.to_string())
            .or_else(|| code.as_str().map(ToOwned::to_owned))?;
        Some(format!("{authority}:{code}"))
    }

    /// Whether the primary column's coordinates are longitude and latitude in degrees.
    ///
    /// This, not an EPSG code, is what a caller needs to know. Real national datasets
    /// are routinely in their own geographic CRS — Colombia's ecosystems map is
    /// EPSG:4686, MAGNA-SIRGAS — so an allow-list containing 4326 would refuse the very
    /// files this exists to read. PROJJSON states the CRS `type` and its axis units
    /// directly, which answers the question for any CRS rather than for a known few.
    ///
    /// A file with no CRS at all counts as geographic: the specification makes that
    /// OGC:CRS84.
    #[must_use]
    pub fn primary_is_geographic(&self) -> bool {
        let Some(Some(crs)) = self.primary().map(|column| column.crs.as_ref()) else {
            return true;
        };
        // A JSON null is how "no CRS" is spelled in practice, and means CRS84 too.
        if crs.is_null() {
            return true;
        }

        if crs.get("type").and_then(serde_json::Value::as_str) != Some("GeographicCRS") {
            return false;
        }

        // A GeographicCRS can still be in gradians or radians. Degrees are what the
        // projection code expects, so anything else has to be refused rather than
        // scaled by a guess.
        crs.get("coordinate_system")
            .and_then(|system| system.get("axis"))
            .and_then(serde_json::Value::as_array)
            .is_none_or(|axes| {
                axes.iter().all(|axis| {
                    axis.get("unit").and_then(serde_json::Value::as_str) == Some("degree")
                })
            })
    }

    /// Bbox covering columns for the primary column, if the file wrote them.
    ///
    /// Their presence is what makes spatial row-group pruning possible: without them a
    /// reader has no per-row-group geometry bounds and must read everything.
    #[must_use]
    pub fn primary_covering(&self) -> Option<&Covering> {
        self.covering.as_ref()
    }
}

/// One row group, with what is needed to decide whether to read it.
#[derive(Debug, Clone)]
pub struct RowGroup {
    /// Position of this row group in the file.
    pub index: usize,
    /// Rows it contains.
    pub num_rows: i64,
    /// Total compressed size of its column chunks.
    pub compressed_size: i64,
}

/// An opened `GeoParquet` file: the footer, decoded.
#[derive(Debug)]
pub struct GeoParquet {
    metadata: Arc<ParquetMetaData>,
    geo: GeoMetadata,
    row_groups: Vec<RowGroup>,
}

impl GeoParquet {
    /// Total rows across every row group.
    #[must_use]
    pub fn num_rows(&self) -> i64 {
        self.metadata.file_metadata().num_rows()
    }

    /// The row groups, in file order.
    #[must_use]
    pub fn row_groups(&self) -> &[RowGroup] {
        &self.row_groups
    }

    /// The parsed `geo` metadata.
    #[must_use]
    pub fn geo(&self) -> &GeoMetadata {
        &self.geo
    }

    /// The underlying parquet metadata.
    #[must_use]
    pub fn metadata(&self) -> &ParquetMetaData {
        &self.metadata
    }

    /// Metadata for one row group.
    #[must_use]
    pub fn row_group_metadata(&self, index: usize) -> Option<&RowGroupMetaData> {
        self.metadata.row_groups().get(index)
    }
}

/// The outcome of parsing a footer.
#[derive(Debug)]
pub enum Footer {
    /// The footer was complete in the bytes provided.
    ///
    /// Boxed because the decoded metadata is far larger than a byte range, and an
    /// unboxed variant would make every `Footer` that size.
    Complete(Box<GeoParquet>),
    /// The footer is bigger than the bytes provided. Fetch this range and parse again.
    ///
    /// The range always reaches the end of the file, so the result can be passed
    /// straight back to [`parse_footer`].
    NeedMore(Range<u64>),
}

/// The byte range to fetch first when opening a file.
///
/// A footer's own length is in the last eight bytes, so its size is unknown until some
/// of the end has been read. Guessing generously usually turns two requests into one;
/// guessing past the start of the file would be a malformed request, so the range is
/// clamped.
#[must_use]
pub fn footer_range(file_size: u64, prefetch: u64) -> Range<u64> {
    file_size.saturating_sub(prefetch)..file_size
}

/// Parse a footer from the tail of a file.
///
/// `tail` must end at the final byte of the file; `file_size` is the object's total
/// size, used to convert offsets within the tail into absolute ranges.
///
/// # Errors
///
/// See [`FormatError`].
pub fn parse_footer(tail: &[u8], file_size: u64) -> Result<Footer, FormatError> {
    if tail.len() < TRAILER_LEN {
        return Err(FormatError::NotParquet {
            reason: format!(
                "a parquet file is at least {TRAILER_LEN} bytes, and only {} were read",
                tail.len()
            ),
        });
    }

    let trailer = &tail[tail.len() - TRAILER_LEN..];
    let magic = &trailer[4..];
    if magic == MAGIC_ENCRYPTED {
        return Err(FormatError::Encrypted);
    }
    if magic != MAGIC {
        return Err(FormatError::NotParquet {
            reason: format!(
                "the file does not end with the parquet magic PAR1, but with {}",
                describe(magic)
            ),
        });
    }

    let declared = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let footer_len = TRAILER_LEN as u64 + u64::from(declared);
    if footer_len > file_size {
        return Err(FormatError::NotParquet {
            reason: format!(
                "the footer claims {declared} bytes of metadata, more than the \
                 {file_size}-byte file holds"
            ),
        });
    }

    // Not enough was fetched. Name the range that is, rather than guessing again:
    // an off-by-one here would send a reader into an endless fetch loop.
    if footer_len > tail.len() as u64 {
        return Ok(Footer::NeedMore(file_size - footer_len..file_size));
    }

    let start = tail.len()
        - usize::try_from(footer_len).map_err(|_| FormatError::NotParquet {
            reason: "the footer is larger than this platform can address".to_owned(),
        })?;
    let metadata = ParquetMetaDataReader::new()
        .parse_and_finish(&Bytes::copy_from_slice(&tail[start..]))
        .map_err(|error| FormatError::Footer(error.to_string()))?;

    let geo = parse_geo_metadata(&metadata)?;
    let row_groups = metadata
        .row_groups()
        .iter()
        .enumerate()
        .map(|(index, group)| RowGroup {
            index,
            num_rows: group.num_rows(),
            compressed_size: group.compressed_size(),
        })
        .collect();

    Ok(Footer::Complete(Box::new(GeoParquet {
        metadata: Arc::new(metadata),
        geo,
        row_groups,
    })))
}

fn parse_geo_metadata(metadata: &ParquetMetaData) -> Result<GeoMetadata, FormatError> {
    let raw = metadata
        .file_metadata()
        .key_value_metadata()
        .and_then(|pairs| pairs.iter().find(|pair| pair.key == "geo"))
        .and_then(|pair| pair.value.as_ref())
        .ok_or(FormatError::NotGeoParquet)?;

    let mut geo: GeoMetadata = serde_json::from_str(raw)
        .map_err(|error| FormatError::BadGeoMetadata(error.to_string()))?;
    raw.clone_into(&mut geo.raw);

    if !geo.columns.contains_key(&geo.primary_column) {
        return Err(FormatError::BadGeoMetadata(format!(
            "primary_column is `{}`, which the `columns` map does not describe",
            geo.primary_column
        )));
    }

    // Lifted out of the primary column rather than read on demand, so callers never
    // have to know that covering is per-column.
    geo.covering = geo.primary().and_then(|column| {
        column.covering.as_ref().map(|covering| Covering {
            xmin: covering.bbox.xmin.clone(),
            ymin: covering.bbox.ymin.clone(),
            xmax: covering.bbox.xmax.clone(),
            ymax: covering.bbox.ymax.clone(),
        })
    });

    Ok(geo)
}

/// Render bytes for an error message, printing them as text when they are readable.
fn describe(bytes: &[u8]) -> String {
    core::str::from_utf8(bytes).map_or_else(
        |_| format!("{bytes:?}"),
        |text| {
            if text.chars().all(|character| !character.is_control()) {
                format!("{text:?}")
            } else {
                format!("{bytes:?}")
            }
        },
    )
}

/// An axis-aligned bounding box in the file's own CRS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox {
    /// Minimum x.
    pub xmin: f64,
    /// Minimum y.
    pub ymin: f64,
    /// Maximum x.
    pub xmax: f64,
    /// Maximum y.
    pub ymax: f64,
}

impl Bbox {
    /// A box from its corners.
    #[must_use]
    pub const fn new(xmin: f64, ymin: f64, xmax: f64, ymax: f64) -> Self {
        Self {
            xmin,
            ymin,
            xmax,
            ymax,
        }
    }

    /// Whether two boxes share any point, edges included.
    ///
    /// Touching counts as overlapping. Excluding a shared edge would drop features
    /// lying exactly on it, which at a grid boundary is the common case rather than a
    /// rare one.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.xmin <= other.xmax
            && self.xmax >= other.xmin
            && self.ymin <= other.ymax
            && self.ymax >= other.ymin
    }
}

/// What to read from a file.
///
/// Every filter is optional, and an absent filter means "no restriction" rather than
/// "match nothing".
#[derive(Debug, Clone, Default)]
pub struct Query {
    bbox: Option<Bbox>,
    ecosystems: Option<Vec<String>>,
}

impl Query {
    /// Restrict to features whose geometry may intersect this box.
    #[must_use]
    pub fn with_bbox(mut self, bbox: Bbox) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Restrict to these ecosystem codes.
    #[must_use]
    pub fn with_ecosystems<I, S>(mut self, codes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ecosystems = Some(codes.into_iter().map(Into::into).collect());
        self
    }

    /// The bounding-box filter, if any.
    #[must_use]
    pub fn bbox(&self) -> Option<&Bbox> {
        self.bbox.as_ref()
    }

    /// The ecosystem filter, if any.
    #[must_use]
    pub fn ecosystems(&self) -> Option<&[String]> {
        self.ecosystems.as_deref()
    }
}

/// Which row groups to read, and which bytes that needs.
#[derive(Debug, Clone)]
pub struct ReadPlan {
    row_groups: Vec<usize>,
    ranges: Vec<Range<u64>>,
}

impl ReadPlan {
    /// The row groups that survived pruning, in file order.
    #[must_use]
    pub fn row_groups(&self) -> &[usize] {
        &self.row_groups
    }

    /// The byte ranges to fetch, ascending and non-overlapping.
    pub fn ranges(&self) -> impl Iterator<Item = Range<u64>> + '_ {
        self.ranges.iter().cloned()
    }

    /// Total bytes this plan will fetch.
    #[must_use]
    pub fn bytes_to_fetch(&self) -> u64 {
        self.ranges
            .iter()
            .map(|range| range.end - range.start)
            .sum()
    }
}

/// One feature: an ecosystem code and its geometry.
#[derive(Debug, Clone)]
pub struct Feature {
    /// The ecosystem code. Empty when the column held null.
    pub ecosystem: String,
    /// The geometry, one entry per polygon; a `MultiPolygon` yields several.
    pub polygons: Vec<crate::wkb::Polygon>,
}

/// Fetched byte ranges, presented to the parquet reader as if they were a file.
///
/// This is what makes the reader sans-IO. The parquet crate asks for byte ranges; this
/// answers from what was already fetched, and reports honestly when something was not.
/// No socket is opened below this point.
#[derive(Debug, Clone)]
pub struct SparseBytes {
    file_size: u64,
    chunks: Vec<(u64, Bytes)>,
}

impl SparseBytes {
    /// Build from a file size and the ranges that were fetched.
    ///
    /// Each entry is the absolute offset the bytes came from, and the bytes themselves.
    #[must_use]
    pub fn new(file_size: u64, mut chunks: Vec<(u64, Bytes)>) -> Self {
        chunks.sort_by_key(|(start, _)| *start);
        Self { file_size, chunks }
    }

    /// Total size of the object these ranges came from.
    #[must_use]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// The fetched bytes running forward from `start`, if that offset was fetched.
    ///
    /// Stops at the end of whichever chunk contains `start`, so a caller reading
    /// forward never crosses silently from fetched bytes into a gap.
    #[must_use]
    pub fn contiguous_from(&self, start: u64) -> Option<&[u8]> {
        self.chunks.iter().find_map(|(offset, bytes)| {
            let end = offset + bytes.len() as u64;
            (start >= *offset && start < end).then(|| {
                let from = usize::try_from(start - offset).ok()?;
                bytes.get(from..)
            })?
        })
    }

    fn slice(&self, start: u64, length: usize) -> Result<Bytes, FormatError> {
        let end = start + length as u64;
        for (offset, bytes) in &self.chunks {
            let chunk_end = offset + bytes.len() as u64;
            if start >= *offset && end <= chunk_end {
                let from = usize::try_from(start - offset)
                    .map_err(|_| FormatError::MissingBytes { start, end })?;
                return Ok(bytes.slice(from..from + length));
            }
        }
        Err(FormatError::MissingBytes { start, end })
    }
}

impl parquet::file::reader::Length for SparseBytes {
    fn len(&self) -> u64 {
        self.file_size
    }
}

impl parquet::file::reader::ChunkReader for SparseBytes {
    type T = bytes::buf::Reader<Bytes>;

    fn get_read(&self, start: u64) -> parquet::errors::Result<Self::T> {
        use bytes::Buf;
        // Serve to the end of whichever fetched chunk contains `start`. The reader only
        // reads forward within a column chunk, so a chunk boundary is a safe stop.
        for (offset, bytes) in &self.chunks {
            let chunk_end = offset + bytes.len() as u64;
            if start >= *offset && start < chunk_end {
                let Ok(from) = usize::try_from(start - offset) else {
                    continue;
                };
                return Ok(bytes.slice(from..).reader());
            }
        }
        Err(parquet::errors::ParquetError::General(format!(
            "bytes at {start} were not fetched"
        )))
    }

    fn get_bytes(&self, start: u64, length: usize) -> parquet::errors::Result<Bytes> {
        self.slice(start, length)
            .map_err(|error| parquet::errors::ParquetError::General(error.to_string()))
    }
}

impl GeoParquet {
    /// Decide which row groups to read and which bytes that needs.
    ///
    /// Pruning is **conservative**: a row group is skipped only when the statistics
    /// prove it cannot match. Missing statistics, or a file without bbox covering
    /// columns, mean the row group is kept. The failure mode of the alternative is an
    /// assessment that silently finds no ecosystem anywhere.
    ///
    /// # Errors
    ///
    /// [`FormatError::NoSuchColumn`] if `ecosystem_column` is not in the file.
    pub fn plan(&self, query: &Query, ecosystem_column: &str) -> Result<ReadPlan, FormatError> {
        let eco_leaf = self.leaf_index(ecosystem_column)?;
        let geometry_leaf = self.leaf_index(&self.geo.primary_column)?;

        // Only the two columns an assessment reads. Fetching the rest would still give
        // correct answers, which is exactly why this needs asserting rather than eyeing.
        let projection = [eco_leaf, geometry_leaf];

        let covering = self
            .geo
            .primary_covering()
            .and_then(|covering| self.covering_leaves(covering));

        let mut row_groups = Vec::new();
        let mut ranges: Vec<Range<u64>> = Vec::new();

        for (index, group) in self.metadata.row_groups().iter().enumerate() {
            if !Self::keeps(group, query, eco_leaf, covering.as_ref()) {
                continue;
            }
            row_groups.push(index);
            for &leaf in &projection {
                let (start, length) = group.column(leaf).byte_range();
                ranges.push(start..start + length);
            }
        }

        Ok(ReadPlan {
            row_groups,
            ranges: coalesce(ranges),
        })
    }

    /// Check the metadata against the file it describes.
    ///
    /// The published `GeoParquet` JSON Schemas validate the `geo` blob's *shape*: that
    /// `primary_column` is a non-empty string, that `encoding` is one of a known set.
    /// They cannot check any of it against the parquet file it sits in, because they
    /// never see it. That gap is where the problems live — a file can be perfectly
    /// schema-valid and name a geometry column that does not exist.
    ///
    /// Findings are advisory. A file that trips several of these is usually still
    /// readable, and refusing to read it would block real work to no purpose.
    #[must_use]
    pub fn check_structure(&self) -> Vec<Finding> {
        let mut findings = Vec::new();
        let schema = self.metadata.file_metadata().schema_descr();
        let columns: Vec<String> = schema
            .columns()
            .iter()
            .map(|column| column.path().string())
            .collect();
        let has = |name: &str| columns.iter().any(|column| column == name);

        let mut error = |message: String| {
            findings.push(Finding {
                severity: Severity::Error,
                message,
            });
        };

        // Every declared geometry column should be a column of the file. The primary
        // one matters most, but a stale entry for a dropped column is worth knowing.
        for name in self.geo.columns.keys() {
            if !has(name) {
                error(format!(
                    "the `geo` metadata describes a geometry column `{name}`, which \
                     this file does not have; it has: {}",
                    columns.join(", ")
                ));
            }
        }

        if !self.geo.primary_is_wkb() {
            error(format!(
                "the geometry column is encoded as `{}`, and this reader decodes only \
                 WKB",
                self.geo.primary_encoding()
            ));
        }

        if !self.geo.primary_is_geographic() {
            error(format!(
                "the geometry is in {}, which is projected; the metrics need \
                 longitude/latitude in degrees, so reproject before assessing",
                self.geo
                    .primary_crs_code()
                    .unwrap_or_else(|| "a projected coordinate system".to_owned())
            ));
        }

        match self.geo.primary_covering() {
            Some(covering) => {
                for path in [
                    &covering.xmin,
                    &covering.ymin,
                    &covering.xmax,
                    &covering.ymax,
                ] {
                    let name = path.join(".");
                    if !has(&name) {
                        // Silently fatal to performance rather than correctness: with
                        // the columns missing the reader cannot prune, so it reads
                        // everything and gives right answers slowly, with nothing to
                        // say why.
                        error(format!(
                            "the `geo` metadata points at a bounding-box column \
                             `{name}`, which this file does not have, so spatial \
                             pruning is impossible and every row group must be read"
                        ));
                    }
                }
            }
            None => findings.push(Finding {
                severity: Severity::Note,
                message: "this file has no bbox covering columns, so a spatial query \
                          must read every row group; writing them makes regional reads \
                          much cheaper"
                    .to_owned(),
            }),
        }

        if self.geo.primary_covering().is_some() && self.geo.version.starts_with("1.0") {
            findings.push(Finding {
                severity: Severity::Note,
                message: format!(
                    "this file declares GeoParquet {} but uses `covering`, which was \
                     introduced in 1.1; that is permitted, and a reader that trusted \
                     the declared version would discard the bounding-box columns and \
                     lose all spatial pruning",
                    self.geo.version
                ),
            });
        }

        if self.geo.primary_crs_code().is_none() {
            findings.push(Finding {
                severity: Severity::Note,
                message: "this file states no CRS, which the specification defines as \
                          OGC:CRS84 — longitude/latitude on WGS84"
                    .to_owned(),
            });
        }

        // Most serious first: this output is meant to be read top-down and acted on.
        findings.sort_by_key(|finding| finding.severity);
        findings
    }

    /// Validate the `geo` metadata against the schema published for its version.
    ///
    /// Says the metadata is well-formed per the specification, and nothing about
    /// whether it matches the file it sits in — a schema never sees the parquet around
    /// it. Pair it with [`Self::check_structure`], which covers exactly that gap.
    ///
    /// Needs the `schema-validation` feature. Never touches the network: the published
    /// schemas, and the PROJJSON schemas they reference, are vendored.
    #[cfg(feature = "schema-validation")]
    #[must_use]
    pub fn validate_against_schema(&self) -> Vec<Finding> {
        crate::schema::validate(&self.geo, &self.geo.raw)
    }

    /// Check that the columns an assessment reads use a codec this build can decode.
    ///
    /// Worth doing from the footer, because the alternative is discovering it after
    /// fetching a row group — and on a national file that is a long wait for a message
    /// that does not say what to do.
    ///
    /// # Errors
    ///
    /// [`FormatError::UnsupportedCompression`], or [`FormatError::NoSuchColumn`] if
    /// `ecosystem_column` is not in the file.
    pub fn check_compression(&self, ecosystem_column: &str) -> Result<(), FormatError> {
        let leaves = [
            self.leaf_index(ecosystem_column)?,
            self.leaf_index(&self.geo.primary_column)?,
        ];

        for group in self.metadata.row_groups() {
            for leaf in leaves {
                let codec = group.column(leaf).compression();
                if let Some(reason) = unavailable(codec) {
                    return Err(FormatError::UnsupportedCompression {
                        codec: format!("{codec:?}"),
                        reason,
                    });
                }
            }
        }
        Ok(())
    }

    /// The byte ranges one row group needs, for the columns an assessment reads.
    ///
    /// Separate from [`Self::plan`] because streaming wants them a row group at a time:
    /// fetching every range a plan names would put the whole selection in memory at
    /// once, which is the thing this design exists to avoid.
    ///
    /// # Errors
    ///
    /// [`FormatError::NoSuchColumn`] if `ecosystem_column` is not in the file.
    pub fn ranges_for_row_group(
        &self,
        index: usize,
        ecosystem_column: &str,
    ) -> Result<Vec<Range<u64>>, FormatError> {
        let eco_leaf = self.leaf_index(ecosystem_column)?;
        let geometry_leaf = self.leaf_index(&self.geo.primary_column)?;
        let Some(group) = self.metadata.row_groups().get(index) else {
            return Ok(Vec::new());
        };

        let ranges = [eco_leaf, geometry_leaf]
            .into_iter()
            .map(|leaf| {
                let (start, length) = group.column(leaf).byte_range();
                start..start + length
            })
            .collect();
        Ok(coalesce(ranges))
    }

    /// Decode one row group, handing each feature to `visit` and then dropping it.
    ///
    /// Returns how many features were visited.
    ///
    /// The callback exists for memory rather than style. A row group of Colombia's
    /// national ecosystems map holds 10,000 features and 114 MB of geometry;
    /// materialising all of them costs that again in decoded coordinates, plus an
    /// allocation per ring, for data each caller folds into an accumulator and
    /// immediately discards. Visiting them one at a time keeps the decoded working set
    /// to a single feature.
    ///
    /// Features with no geometry are skipped: they can contribute to neither metric,
    /// and counting them would add an ecosystem with no area.
    ///
    /// # Errors
    ///
    /// [`FormatError::MissingBytes`] if the plan's ranges were not all supplied, and
    /// [`FormatError::Geometry`] if the WKB does not decode.
    pub fn for_each_feature<F>(
        &self,
        index: usize,
        bytes: &SparseBytes,
        ecosystem_column: &str,
        mut visit: F,
    ) -> Result<usize, FormatError>
    where
        F: FnMut(&str, &[crate::wkb::Polygon]),
    {
        use parquet::arrow::arrow_reader::{
            ArrowReaderMetadata, ArrowReaderOptions, ParquetRecordBatchReaderBuilder,
        };
        use parquet::arrow::ProjectionMask;

        let eco_leaf = self.leaf_index(ecosystem_column)?;
        let geometry_leaf = self.leaf_index(&self.geo.primary_column)?;
        let schema = self.metadata.file_metadata().schema_descr();

        let reader_metadata =
            ArrowReaderMetadata::try_new(Arc::clone(&self.metadata), ArrowReaderOptions::default())
                .map_err(|error| FormatError::Read(error.to_string()))?;

        let reader =
            ParquetRecordBatchReaderBuilder::new_with_metadata(bytes.clone(), reader_metadata)
                .with_row_groups(vec![index])
                .with_projection(ProjectionMask::leaves(schema, [eco_leaf, geometry_leaf]))
                .build()
                .map_err(|error| FormatError::Read(error.to_string()))?;

        let mut visited = 0;
        for batch in reader {
            let batch = batch.map_err(|error| FormatError::Read(error.to_string()))?;
            let codes = string_column(&batch, ecosystem_column)?;
            let geometry = binary_column(&batch, &self.geo.primary_column)?;

            for row in 0..batch.num_rows() {
                let Some(wkb) = geometry.get(row).copied().flatten() else {
                    // No geometry means nothing to contribute to either metric. Keeping
                    // the row would add an ecosystem with no area.
                    continue;
                };
                let polygons =
                    crate::wkb::decode_polygons(wkb).map_err(|source| FormatError::Geometry {
                        column: self.geo.primary_column.clone(),
                        source,
                    })?;
                visit(codes.get(row).copied().flatten().unwrap_or(""), &polygons);
                visited += 1;
                // `polygons` is dropped here. Holding every feature of a row group
                // instead costs hundreds of megabytes on a national file, for data
                // that is folded into an accumulator and immediately finished with.
            }
        }
        Ok(visited)
    }

    /// Decode one row group into a vector of features.
    ///
    /// Convenient, and it holds every feature of the row group at once. Prefer
    /// [`Self::for_each_feature`] for anything large.
    ///
    /// # Errors
    ///
    /// As [`Self::for_each_feature`].
    pub fn decode_row_group(
        &self,
        index: usize,
        bytes: &SparseBytes,
        ecosystem_column: &str,
    ) -> Result<Vec<Feature>, FormatError> {
        let mut features = Vec::new();
        self.for_each_feature(index, bytes, ecosystem_column, |ecosystem, polygons| {
            features.push(Feature {
                ecosystem: ecosystem.to_owned(),
                polygons: polygons.to_vec(),
            });
        })?;
        Ok(features)
    }

    /// Whether a row group could contain a match.
    fn keeps(
        group: &RowGroupMetaData,
        query: &Query,
        eco_leaf: usize,
        covering: Option<&CoveringLeaves>,
    ) -> bool {
        if let Some(wanted) = query.ecosystems() {
            // Parquet orders string statistics by unsigned byte comparison, which is
            // exactly Rust's ordering for `str`. A code outside [min, max] cannot be in
            // this row group; one inside might be, which is all pruning needs.
            if let (Some(min), Some(max)) = (
                string_stat(group, eco_leaf, Bound::Min),
                string_stat(group, eco_leaf, Bound::Max),
            ) {
                if !wanted
                    .iter()
                    .any(|code| code.as_str() >= min.as_str() && code.as_str() <= max.as_str())
                {
                    return false;
                }
            }
        }

        if let (Some(wanted), Some(leaves)) = (query.bbox(), covering) {
            if let Some(bounds) = leaves.bounds(group) {
                if !bounds.intersects(wanted) {
                    return false;
                }
            }
        }

        true
    }

    fn leaf_index(&self, name: &str) -> Result<usize, FormatError> {
        let schema = self.metadata.file_metadata().schema_descr();
        schema
            .columns()
            .iter()
            .position(|column| column.path().string() == name || column.name() == name)
            .ok_or_else(|| FormatError::NoSuchColumn {
                wanted: name.to_owned(),
                available: schema
                    .columns()
                    .iter()
                    .map(|column| column.path().string())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
    }

    fn covering_leaves(&self, covering: &Covering) -> Option<CoveringLeaves> {
        Some(CoveringLeaves {
            xmin: self.leaf_index(&covering.xmin.join(".")).ok()?,
            ymin: self.leaf_index(&covering.ymin.join(".")).ok()?,
            xmax: self.leaf_index(&covering.xmax.join(".")).ok()?,
            ymax: self.leaf_index(&covering.ymax.join(".")).ok()?,
        })
    }
}

/// Leaf column indices of the four bbox covering columns.
#[derive(Debug, Clone, Copy)]
struct CoveringLeaves {
    xmin: usize,
    ymin: usize,
    xmax: usize,
    ymax: usize,
}

impl CoveringLeaves {
    /// The bounding box of every geometry in a row group.
    ///
    /// Note which statistic each corner uses: the row group's western edge is the
    /// *minimum* of the per-feature `xmin` column, and its eastern edge the *maximum* of
    /// the `xmax` column. Taking both from one column would give a box that is too
    /// small and would prune away row groups that do overlap.
    fn bounds(&self, group: &RowGroupMetaData) -> Option<Bbox> {
        Some(Bbox {
            xmin: double_stat(group, self.xmin, Bound::Min)?,
            ymin: double_stat(group, self.ymin, Bound::Min)?,
            xmax: double_stat(group, self.xmax, Bound::Max)?,
            ymax: double_stat(group, self.ymax, Bound::Max)?,
        })
    }
}

#[derive(Clone, Copy)]
enum Bound {
    Min,
    Max,
}

fn double_stat(group: &RowGroupMetaData, leaf: usize, bound: Bound) -> Option<f64> {
    use parquet::file::statistics::Statistics;
    match group.column(leaf).statistics()? {
        Statistics::Double(stats) => match bound {
            Bound::Min => stats.min_opt().copied(),
            Bound::Max => stats.max_opt().copied(),
        },
        Statistics::Float(stats) => match bound {
            Bound::Min => stats.min_opt().copied().map(f64::from),
            Bound::Max => stats.max_opt().copied().map(f64::from),
        },
        _ => None,
    }
}

fn string_stat(group: &RowGroupMetaData, leaf: usize, bound: Bound) -> Option<String> {
    use parquet::file::statistics::Statistics;
    let value = match group.column(leaf).statistics()? {
        Statistics::ByteArray(stats) => match bound {
            Bound::Min => stats.min_opt()?.data().to_vec(),
            Bound::Max => stats.max_opt()?.data().to_vec(),
        },
        _ => return None,
    };
    String::from_utf8(value).ok()
}

/// Merge ranges that touch or overlap, leaving genuine gaps alone.
///
/// Only exact adjacency is merged. Bridging a gap would pull in the columns between —
/// the very ones the projection exists to avoid.
fn coalesce(mut ranges: Vec<Range<u64>>) -> Vec<Range<u64>> {
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<u64>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// Borrow a string column as `Option<&str>` per row, whatever its Arrow flavour.
fn string_column<'a>(
    batch: &'a arrow_array::RecordBatch,
    name: &str,
) -> Result<Vec<Option<&'a str>>, FormatError> {
    use arrow_array::{cast::AsArray, types::GenericStringType, Array};

    let column = batch
        .column_by_name(name)
        .ok_or_else(|| FormatError::NoSuchColumn {
            wanted: name.to_owned(),
            available: batch
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect::<Vec<_>>()
                .join(", "),
        })?;

    if let Some(array) = column.as_bytes_opt::<GenericStringType<i32>>() {
        return Ok((0..array.len())
            .map(|row| (!array.is_null(row)).then(|| array.value(row)))
            .collect());
    }
    if let Some(array) = column.as_bytes_opt::<GenericStringType<i64>>() {
        return Ok((0..array.len())
            .map(|row| (!array.is_null(row)).then(|| array.value(row)))
            .collect());
    }
    Err(FormatError::UnexpectedType {
        column: name.to_owned(),
        found: column.data_type().to_string(),
        expected: "a string column",
    })
}

/// Borrow a binary column as `Option<&[u8]>` per row.
fn binary_column<'a>(
    batch: &'a arrow_array::RecordBatch,
    name: &str,
) -> Result<Vec<Option<&'a [u8]>>, FormatError> {
    use arrow_array::{cast::AsArray, types::GenericBinaryType, Array};

    let column = batch
        .column_by_name(name)
        .ok_or_else(|| FormatError::NoSuchColumn {
            wanted: name.to_owned(),
            available: batch
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect::<Vec<_>>()
                .join(", "),
        })?;

    if let Some(array) = column.as_bytes_opt::<GenericBinaryType<i32>>() {
        return Ok((0..array.len())
            .map(|row| (!array.is_null(row)).then(|| array.value(row)))
            .collect());
    }
    if let Some(array) = column.as_bytes_opt::<GenericBinaryType<i64>>() {
        return Ok((0..array.len())
            .map(|row| (!array.is_null(row)).then(|| array.value(row)))
            .collect());
    }
    Err(FormatError::UnexpectedType {
        column: name.to_owned(),
        found: column.data_type().to_string(),
        expected: "a binary column holding WKB",
    })
}

/// Why a codec cannot be decoded by this build, or `None` if it can.
///
/// The split is by implementation language, not preference. Snappy, gzip, brotli and
/// LZ4 are pure Rust and go everywhere the rest of this crate goes. ZSTD binds a C
/// library whose build fails for `wasm32-unknown-unknown`, so it is compiled in for
/// native targets only — which matters, because ZSTD is what real `GeoParquet` is
/// actually written with.
fn unavailable(codec: parquet::basic::Compression) -> Option<&'static str> {
    use parquet::basic::Compression;

    match codec {
        Compression::ZSTD(_) if cfg!(target_arch = "wasm32") => Some(
            "ZSTD needs a C library that cannot be compiled to WebAssembly, so the \
             browser build omits it; read this file from a native build, or rewrite it \
             with snappy or gzip compression",
        ),
        Compression::LZO => Some(
            "LZO is not implemented by the parquet reader this library uses; rewrite \
             the file with snappy, gzip or zstd compression",
        ),
        _ => None,
    }
}
