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
    footer_range, parse_footer, Footer, FormatError, GeoParquet, Query, SparseBytes,
    DEFAULT_FOOTER_PREFETCH,
};
use iucn_rle_io::ByteSource;

/// Something that went wrong reading a dataset.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Fetching bytes failed.
    #[error(transparent)]
    Io(#[from] iucn_rle_io::IoError),

    /// Decoding failed.
    #[error(transparent)]
    Format(#[from] FormatError),

    /// The file's geometry is not in a coordinate system the metrics accept.
    #[error(
        "this dataset is in {found}, but the metrics need longitude/latitude on WGS84 \
         (EPSG:4326 or OGC:CRS84); reproject it before assessing"
    )]
    NotGeographic {
        /// The CRS the file declared.
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadReport {
    /// Row groups that were fetched and decoded.
    pub row_groups_read: usize,
    /// Row groups skipped because the footer's statistics proved they could not match.
    pub row_groups_skipped: usize,
    /// Features fed to the accumulator.
    pub features: usize,
    /// Bytes fetched, footer included.
    pub bytes_fetched: u64,
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

    let plan = file.plan(query, ecosystem_column)?;
    report.row_groups_skipped = file.row_groups().len() - plan.row_groups().len();

    for &index in plan.row_groups() {
        let ranges = file.ranges_for_row_group(index, ecosystem_column)?;
        let bytes = fetch(source, &ranges, &mut report).await?;

        for feature in file.decode_row_group(index, &bytes, ecosystem_column)? {
            // A MultiPolygon's parts go in separately: each contributes to the AOO on
            // its own, and merging them into one ring list would read the second part's
            // exterior as a hole in the first.
            for polygon in &feature.polygons {
                accumulator.add_polygon(&feature.ecosystem, polygon);
            }
            report.features += 1;
        }
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
    let size = source.size().await?;
    let mut range = footer_range(size, options.footer_prefetch);

    // Bounded because `NeedMore` always names a range reaching the end of the file, so
    // one retry suffices in practice; the loop guards against a malformed footer that
    // keeps asking rather than trusting it to converge.
    for _ in 0..4 {
        let tail = source.read_range(range.clone()).await?;
        report.bytes_fetched += tail.len() as u64;

        match parse_footer(&tail, size)? {
            Footer::Complete(file) => return Ok(*file),
            Footer::NeedMore(wider) => range = wider,
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

    // The metrics project from longitude/latitude themselves, so an already-projected
    // file would be silently misread as degrees — small numbers, plausible output,
    // entirely wrong. An absent CRS means OGC:CRS84 per the specification, which is
    // what is wanted, so only a *stated* other CRS is refused.
    if let Some(code) = file.geo().primary_crs_code() {
        if !matches!(code.as_str(), "EPSG:4326" | "OGC:CRS84") {
            return Err(EngineError::NotGeographic { found: code });
        }
    }

    // Resolving the column now turns a typo into an immediate error rather than one
    // discovered after the first row group has been fetched.
    file.ranges_for_row_group(0, ecosystem_column)?;
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
