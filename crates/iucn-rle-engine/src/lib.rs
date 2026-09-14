//! Fetch-and-decode orchestration for Red List of Ecosystems assessments.
//!
//! This is the only crate that joins the two halves of the design: `iucn-rle-io`
//! fetches bytes asynchronously, `iucn-rle-format` decodes them synchronously, and the
//! loop below runs between them. Nothing here parses a byte or computes a metric.
//!
//! # Why the loop shape is the point
//!
//! A national dataset is gigabytes and an assessment touches a fraction of it. The
//! saving comes from two things happening in the right order:
//!
//! 1. Prune first — decide from the footer's statistics which row groups can possibly
//!    match, and never fetch the rest.
//! 2. Then stream — fetch one surviving row group, decode it, fold it into the
//!    accumulator, and drop it before fetching the next.
//!
//! Skipping step 2 is the tempting mistake: fetching every range a plan names is
//! simpler, gives identical answers, and puts the entire selection in memory at once.
//! That is precisely the failure this milestone exists to fix, so [`ReadReport`]
//! records what was actually fetched and the tests assert on it.
//!
//! Peak memory is therefore one row group plus the accumulator, and the accumulator
//! grows with *occupied grid cells* rather than with features.

use core::ops::Range;

use iucn_rle_core::distribution::DistributionAccumulator;
use iucn_rle_format::geoparquet::{
    parse_footer, Footer, FormatError, GeoParquet, Query, SparseBytes, DEFAULT_FOOTER_PREFETCH,
};
use iucn_rle_io::ByteSource;

#[cfg(all(feature = "blocking", not(target_arch = "wasm32")))]
pub mod blocking;

/// Something that went wrong reading a dataset.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Fetching bytes failed.
    #[error(transparent)]
    Io(#[from] iucn_rle_io::IoError),

    /// Decoding failed.
    #[error(transparent)]
    Format(#[from] FormatError),

    /// The file's geometry is not in longitude and latitude.
    #[error(
        "this dataset is in {found}, but the metrics need longitude/latitude in \
         degrees; reproject it to a geographic CRS before assessing"
    )]
    NotGeographic {
        /// The CRS the file declared.
        found: String,
    },

    /// Decoding a raster failed.
    #[error(transparent)]
    Cog(#[from] iucn_rle_format::cog::CogError),

    /// The raster is not in the AOO grid's coordinate system.
    #[error(
        "this raster is in {found}, but the AOO grid is defined in ESRI:54034; pixels \
         are only rectangles in that plane, so reproject it before assessing"
    )]
    NotGridCrs {
        /// What the file's projection keys said.
        found: String,
    },

    /// The geometry column uses an encoding this reader does not decode.
    #[error("this dataset stores geometry as {encoding}, and only WKB is supported")]
    UnsupportedEncoding {
        /// The encoding the file declared.
        encoding: String,
    },
}

/// What a read did.
///
/// Exists so the efficiency claim is observable rather than asserted. A reader that
/// downloaded everything and returned the right answer would look identical without it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReadReport {
    /// Row groups that were fetched and decoded.
    pub row_groups_read: usize,
    /// Row groups skipped because the footer's statistics proved they could not match.
    pub row_groups_skipped: usize,
    /// Features fed to the accumulator.
    pub features: usize,
    /// Bytes fetched, footer included.
    pub bytes_fetched: u64,
    /// The coordinate reference system the file declared, if it named one.
    ///
    /// Recorded because the metrics project from longitude/latitude assuming WGS84,
    /// while real data often arrives on a national datum — MAGNA-SIRGAS for Colombia.
    /// The difference is far below the resolution of a 10 km grid, but it is an
    /// assumption, and an assessment's provenance should say which datum it rested on
    /// rather than quietly equating the two.
    pub crs: Option<String>,
}

/// How to read, for callers who know something about their files.
#[derive(Debug, Clone)]
pub struct ReadOptions {
    /// Bytes to fetch when first looking for the footer.
    ///
    /// The default suits files of unknown size. Raise it for very wide datasets, whose
    /// footers carry statistics for every column of every row group; lower it when
    /// files are known to be small, since a prefetch larger than the object simply
    /// fetches the whole thing — which is the right behaviour, but not streaming.
    pub footer_prefetch: u64,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            footer_prefetch: DEFAULT_FOOTER_PREFETCH,
        }
    }
}

/// Read a remote `GeoParquet` dataset into an accumulator, one row group at a time.
///
/// The accumulator is borrowed rather than returned so several datasets can be folded
/// into one assessment — an ecosystem whose distribution spans more than one file is
/// ordinary rather than exceptional.
///
/// # Errors
///
/// See [`EngineError`].
pub async fn accumulate_geoparquet<S: ByteSource + ?Sized>(
    source: &S,
    query: &Query,
    ecosystem_column: &str,
    accumulator: &mut DistributionAccumulator,
) -> Result<ReadReport, EngineError> {
    accumulate_geoparquet_with(
        source,
        query,
        ecosystem_column,
        accumulator,
        &ReadOptions::default(),
    )
    .await
}

/// As [`accumulate_geoparquet`], with control over how the file is read.
///
/// # Errors
///
/// See [`EngineError`]. Structural problems — a missing column, a projected CRS, a
/// geometry encoding this reader cannot decode — are all reported from the footer,
/// before any row group is fetched, so a mistake costs one small request rather than a
/// long download.
pub async fn accumulate_geoparquet_with<S: ByteSource + ?Sized>(
    source: &S,
    query: &Query,
    ecosystem_column: &str,
    accumulator: &mut DistributionAccumulator,
    options: &ReadOptions,
) -> Result<ReadReport, EngineError> {
    let mut report = ReadReport::default();
    let file = open(source, options, &mut report).await?;

    check_readable(&file, ecosystem_column)?;
    report.crs = file.geo().primary_crs_code();

    let plan = file.plan(query, ecosystem_column)?;
    report.row_groups_skipped = file.row_groups().len() - plan.row_groups().len();

    for &index in plan.row_groups() {
        let ranges = file.ranges_for_row_group(index, ecosystem_column)?;
        let bytes = fetch(source, &ranges, &mut report).await?;

        // Streamed feature by feature rather than decoded into a vector: a row group of
        // the Colombia dataset is 114 MB of geometry, and holding its decoded form as
        // well would multiply peak memory for data that is folded in and dropped.
        report.features +=
            file.for_each_feature(index, &bytes, ecosystem_column, |ecosystem, polygons| {
                // A MultiPolygon's parts go in separately: each contributes to the AOO
                // on its own, and merging them into one ring list would read the second
                // part's exterior as a hole in the first.
                for polygon in polygons {
                    accumulator.add_polygon(ecosystem, polygon);
                }
            })?;
        report.row_groups_read += 1;
        // `bytes` is dropped here, before the next row group is fetched. That single
        // fact is what bounds peak memory.
    }

    Ok(report)
}

/// Read and parse the footer, following a `NeedMore` if the first guess was short.
async fn open<S: ByteSource + ?Sized>(
    source: &S,
    options: &ReadOptions,
    report: &mut ReadReport,
) -> Result<GeoParquet, EngineError> {
    // A suffix read, so the very first request returns footer bytes. Asking for the size
    // first — which is what naming an absolute range requires — spends a whole round trip
    // before any data moves, and the response to a suffix request states the total in
    // `Content-Range` anyway. On a remote read that round trip is a real fraction of the
    // wall clock: the work is made of latency far more than of bytes.
    let mut tail = source.read_suffix(options.footer_prefetch).await?;
    report.bytes_fetched += tail.len() as u64;
    // Free: the suffix response carried it, and `HttpSource` cached it.
    let size = source.size().await?;

    // Bounded because `NeedMore` always names a range reaching the end of the file, so
    // one retry suffices in practice; the loop guards against a malformed footer that
    // keeps asking rather than trusting it to converge.
    for _ in 0..4 {
        match parse_footer(&tail, size)? {
            Footer::Complete(file) => return Ok(*file),
            Footer::NeedMore(wider) => {
                tail = source.read_range(wider).await?;
                report.bytes_fetched += tail.len() as u64;
            }
        }
    }

    Err(EngineError::Format(FormatError::Footer(
        "the footer kept asking for more bytes without ever parsing".to_owned(),
    )))
}

/// Check everything that can be known from the footer, before fetching any data.
fn check_readable(file: &GeoParquet, ecosystem_column: &str) -> Result<(), EngineError> {
    if !file.geo().primary_is_wkb() {
        return Err(EngineError::UnsupportedEncoding {
            encoding: file.geo().primary_encoding().to_owned(),
        });
    }

    // The metrics project from longitude/latitude themselves, so a projected file would
    // be misread as degrees — small numbers, plausible output, entirely wrong.
    //
    // The test is whether coordinates are degrees, *not* whether the code is 4326.
    // National datasets are routinely on their own geographic datum, and this reader
    // exists to read them: an allow-list of one code refuses Colombia's ecosystems map
    // (EPSG:4686) outright. Which datum was used is recorded in the report instead.
    if !file.geo().primary_is_geographic() {
        return Err(EngineError::NotGeographic {
            found: file
                .geo()
                .primary_crs_code()
                .unwrap_or_else(|| "a projected coordinate system".to_owned()),
        });
    }

    // Resolving the column now turns a typo into an immediate error rather than one
    // discovered after the first row group has been fetched. The same goes for a codec
    // this build lacks.
    file.ranges_for_row_group(0, ecosystem_column)?;
    file.check_compression(ecosystem_column)?;
    Ok(())
}

/// Fetch byte ranges and record what they cost.
async fn fetch<S: ByteSource + ?Sized>(
    source: &S,
    ranges: &[Range<u64>],
    report: &mut ReadReport,
) -> Result<SparseBytes, EngineError> {
    let size = source.size().await?;
    let parts = source.read_ranges(ranges).await?;

    let chunks: Vec<_> = ranges
        .iter()
        .map(|range| range.start)
        .zip(parts)
        .inspect(|(_, bytes)| report.bytes_fetched += bytes.len() as u64)
        .collect();

    Ok(SparseBytes::new(size, chunks))
}

/// Advice for a URL that failed to read, if its shape suggests a likely cause.
///
/// Data portals commonly serve a browsable web interface and the bytes themselves from
/// different hosts, at otherwise identical paths. Pasting the address bar's URL is the
/// easiest mistake to make, and the resulting failure — an HTML page that does not
/// honour range requests — describes a symptom rather than the cause.
#[must_use]
pub fn url_hint(url: &str) -> Option<String> {
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or_default();

    // source.coop is where this project's own datasets live, so the specific case is
    // worth naming rather than gesturing at generically.
    if host.eq_ignore_ascii_case("source.coop") {
        return Some(format!(
            "source.coop serves its web interface at this address, and the files \
             themselves from data.source.coop. Try:\n  {}",
            url.replacen("source.coop", "data.source.coop", 1)
        ));
    }

    if host.starts_with("www.") || url.contains("/blob/") {
        return Some(
            "this looks like a web page rather than the file itself; look for a direct \
             download or raw link"
                .to_owned(),
        );
    }

    None
}

/// What a raster read did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RasterReport {
    /// Tiles fetched and decoded.
    pub tiles_read: usize,
    /// Bytes fetched, header included.
    pub bytes_fetched: u64,
    /// The largest single tile fetched — what bounds peak memory.
    pub peak_tile_bytes: u64,
}

/// Read a remote Cloud-Optimized `GeoTIFF` into an AOO accumulator, a tile at a time.
///
/// `class` is the pixel value identifying the ecosystem; pixels holding it count as
/// fully covered and everything else as empty, which is what a categorical map means.
///
/// The header is fetched once and reused, so each iteration transfers one tile and
/// holds one decoded window. Peak memory is a tile, not a raster.
///
/// # Errors
///
/// [`EngineError::NotGridCrs`] if the raster is not in the AOO grid's CRS — refused
/// rather than approximated, since pixels stop being rectangles in any other plane.
pub async fn accumulate_cog<S: ByteSource + ?Sized>(
    source: &S,
    ecosystem: &str,
    class: f64,
    accumulator: &mut iucn_rle_core::aoo::AooAccumulator,
) -> Result<RasterReport, EngineError> {
    use iucn_rle_format::cog::{header_range, parse_header, Header, DEFAULT_HEADER_PREFETCH};

    let mut report = RasterReport::default();
    let size = source.size().await?;

    let mut range = header_range(size, DEFAULT_HEADER_PREFETCH);
    let (cog, header_bytes) = loop {
        let head = source.read_range(range.clone()).await?;
        report.bytes_fetched += head.len() as u64;
        match parse_header(&head, size)? {
            Header::Complete(cog) => break (cog, head),
            Header::NeedMore(wider) => range = wider,
        }
    };

    if !cog.is_aoo_crs() {
        return Err(EngineError::NotGridCrs {
            found: cog.crs_description(),
        });
    }

    for tile in 0..cog.tile_count() {
        let Some(tile_range) = cog.tile_byte_range(tile) else {
            continue;
        };
        let tile_bytes = source.read_range(tile_range.clone()).await?;
        report.bytes_fetched += tile_bytes.len() as u64;
        report.peak_tile_bytes = report.peak_tile_bytes.max(tile_bytes.len() as u64);

        // The header is reused rather than refetched: the decoder re-reads the IFD to
        // find the tile, and those bytes are already in hand.
        let fetched = SparseBytes::new(
            size,
            vec![(0, header_bytes.clone()), (tile_range.start, tile_bytes)],
        );
        let raster = cog.decode(cog.tile_window(tile), &fetched)?;
        let coverage = raster.mask(class);
        accumulator.add_raster(ecosystem, &raster.as_window(&coverage));
        report.tiles_read += 1;
        // The decoded tile and its mask are dropped here, before the next is fetched.
    }

    Ok(report)
}
