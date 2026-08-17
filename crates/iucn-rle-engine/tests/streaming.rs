//! Reading a remote dataset into an assessment without holding it in memory.
//!
//! The claim this milestone rests on is not "it produces the right numbers" — the
//! format crate's tests cover that — but "it produces them while holding one row group
//! at a time". So these tests assert on *how* the data was read as much as on what came
//! out: how many row groups were touched, how many bytes were fetched, and how much was
//! ever in flight at once.

#![allow(clippy::cast_possible_truncation)]

use std::cell::RefCell;
use std::fs;
use std::ops::Range;
use std::path::PathBuf;

use bytes::Bytes;
use futures::executor::block_on;
use iucn_rle_core::distribution::DistributionAccumulator;
use iucn_rle_engine::{
    accumulate_geoparquet, accumulate_geoparquet_with, EngineError, ReadOptions,
};
use iucn_rle_format::geoparquet::{Bbox, Query};
use iucn_rle_io::{ByteSource, InMemorySource, IoError};

const ECO_COLUMN: &str = "eco_code";

/// The fixture whose row groups outweigh its footer.
///
/// `ecosystems.parquet` cannot demonstrate streaming and it took a failing test to see
/// why: its footer is about two thirds of the file, because parquet metadata has a
/// floor — the `geo` key alone carries a full PROJJSON CRS. On a file that small,
/// "fetch the footer" is very nearly "fetch everything" whether a reader streams or
/// not. This one has 2000 features in 8 row groups, so the two are distinguishable.
const MANY: &str = "ecosystems_many.parquet";

/// A footer prefetch sized for the streaming fixture rather than for unknown files.
///
/// The 64 KB default is a third of that fixture, which would swamp the measurement
/// while being entirely negligible on the national datasets the default is for.
const SMALL_PREFETCH: ReadOptions = ReadOptions {
    footer_prefetch: 16_384,
};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/data")
        .join(name);
    fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}. Regenerate with \
             `python3 tools/generate_geoparquet_fixture.py`",
            path.display()
        )
    })
}

/// A source that records what was asked of it.
///
/// Wrapping rather than mocking: the bytes come from the real file through the real
/// `InMemorySource`, and this only watches. A mock would let the engine pass while
/// reading nothing.
struct Watched {
    inner: InMemorySource,
    log: RefCell<Vec<u64>>,
    largest_single_fetch: RefCell<u64>,
}

impl Watched {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            inner: InMemorySource::new(bytes),
            log: RefCell::new(Vec::new()),
            largest_single_fetch: RefCell::new(0),
        }
    }

    fn bytes_fetched(&self) -> u64 {
        self.log.borrow().iter().sum()
    }

    /// The most bytes returned by any one call — the peak the engine had in hand.
    fn peak_in_flight(&self) -> u64 {
        *self.largest_single_fetch.borrow()
    }
}

#[async_trait::async_trait(?Send)]
impl ByteSource for Watched {
    async fn size(&self) -> Result<u64, IoError> {
        self.inner.size().await
    }

    async fn read_range(&self, range: Range<u64>) -> Result<Bytes, IoError> {
        let bytes = self.inner.read_range(range).await?;
        self.log.borrow_mut().push(bytes.len() as u64);
        let mut peak = self.largest_single_fetch.borrow_mut();
        *peak = (*peak).max(bytes.len() as u64);
        Ok(bytes)
    }

    async fn read_ranges(&self, ranges: &[Range<u64>]) -> Result<Vec<Bytes>, IoError> {
        let parts = self.inner.read_ranges(ranges).await?;
        let total: u64 = parts.iter().map(|part| part.len() as u64).sum();
        self.log.borrow_mut().push(total);
        let mut peak = self.largest_single_fetch.borrow_mut();
        *peak = (*peak).max(total);
        Ok(parts)
    }
}

#[test]
fn every_feature_is_read_into_the_accumulator() {
    let source = Watched::new(fixture("ecosystems.parquet"));
    let mut accumulator = DistributionAccumulator::new();

    let report = block_on(accumulate_geoparquet(
        &source,
        &Query::default(),
        ECO_COLUMN,
        &mut accumulator,
    ))
    .unwrap();

    assert_eq!(report.features, 16);
    assert_eq!(report.row_groups_read, 4);
    assert_eq!(report.row_groups_skipped, 0);

    let distribution = accumulator.finish();
    assert_eq!(
        distribution.ecosystems(),
        ["ECO_A", "ECO_B", "ECO_C", "ECO_D"]
    );
}

#[test]
fn the_metrics_are_real_numbers_not_zeroes() {
    // A loop that fetched nothing would still report success and leave an empty
    // accumulator, so the numbers have to be checked as well as the counts.
    let source = InMemorySource::new(fixture("ecosystems.parquet"));
    let mut accumulator = DistributionAccumulator::new();

    block_on(accumulate_geoparquet(
        &source,
        &Query::default().with_ecosystems(["ECO_C"]),
        ECO_COLUMN,
        &mut accumulator,
    ))
    .unwrap();

    let distribution = accumulator.finish();
    assert!(
        distribution.eoo_km2("ECO_C") > 0.0,
        "ECO_C spans several degrees, so its EOO cannot be zero"
    );
    assert!(distribution.aoo("ECO_C").aoo_cells > 0);
}

#[test]
fn a_filtered_query_skips_the_row_groups_it_proved_irrelevant() {
    let source = Watched::new(fixture("ecosystems.parquet"));
    let mut accumulator = DistributionAccumulator::new();

    let report = block_on(accumulate_geoparquet(
        &source,
        &Query::default().with_ecosystems(["ECO_B"]),
        ECO_COLUMN,
        &mut accumulator,
    ))
    .unwrap();

    assert_eq!(report.row_groups_read, 1);
    assert_eq!(report.row_groups_skipped, 3);
    assert_eq!(report.features, 4);
}

#[test]
fn a_filtered_query_fetches_far_less_than_the_whole_file() {
    let source = Watched::new(fixture(MANY));
    let mut accumulator = DistributionAccumulator::new();

    block_on(accumulate_geoparquet_with(
        &source,
        &Query::default().with_ecosystems(["MANY_03"]),
        ECO_COLUMN,
        &mut accumulator,
        &SMALL_PREFETCH,
    ))
    .unwrap();

    let whole_file = fixture(MANY).len() as u64;
    assert!(
        source.bytes_fetched() * 2 < whole_file,
        "fetched {} of {whole_file} bytes",
        source.bytes_fetched()
    );
}

#[test]
fn no_more_than_one_row_group_is_ever_in_flight() {
    // The memory claim, made checkable. Reading every row group must still never hold
    // more than one at a time — that is the whole difference from loading the file.
    let bytes = fixture(MANY);
    let source = Watched::new(bytes.clone());
    let mut accumulator = DistributionAccumulator::new();

    let report = block_on(accumulate_geoparquet_with(
        &source,
        &Query::default(),
        ECO_COLUMN,
        &mut accumulator,
        &SMALL_PREFETCH,
    ))
    .unwrap();

    assert_eq!(report.row_groups_read, 8, "every row group was read");
    assert_eq!(report.features, 2_000, "and every feature");
    // Having read the whole file, no single fetch may approach its size. A reader that
    // gathered every range before decoding would fail here while returning identical
    // numbers.
    assert!(
        source.peak_in_flight() * 4 < bytes.len() as u64,
        "peak fetch was {} bytes of a {}-byte file, which is not streaming",
        source.peak_in_flight(),
        bytes.len()
    );
}

#[test]
fn a_bounding_box_query_reads_only_the_overlapping_row_group() {
    let source = Watched::new(fixture("ecosystems.parquet"));
    let mut accumulator = DistributionAccumulator::new();

    let report = block_on(accumulate_geoparquet(
        &source,
        &Query::default().with_bbox(Bbox::new(-1.0, -1.0, 10.0, 5.0)),
        ECO_COLUMN,
        &mut accumulator,
    ))
    .unwrap();

    assert_eq!(report.row_groups_read, 1);
    assert_eq!(report.features, 4);
}

#[test]
fn a_query_matching_nothing_succeeds_with_an_empty_result() {
    // A valid question about a region an ecosystem does not reach. Not an error.
    let source = InMemorySource::new(fixture("ecosystems.parquet"));
    let mut accumulator = DistributionAccumulator::new();

    let report = block_on(accumulate_geoparquet(
        &source,
        &Query::default().with_ecosystems(["ECO_NOWHERE"]),
        ECO_COLUMN,
        &mut accumulator,
    ))
    .unwrap();

    assert_eq!(report.features, 0);
    assert_eq!(report.row_groups_read, 0);
    assert!(accumulator.finish().ecosystems().is_empty());
}

#[test]
fn a_missing_ecosystem_column_is_reported_before_any_data_is_fetched() {
    // Failing fast matters: on a national file the alternative is a long download that
    // ends in the same error.
    let source = Watched::new(fixture("ecosystems.parquet"));
    let mut accumulator = DistributionAccumulator::new();

    let error = block_on(accumulate_geoparquet(
        &source,
        &Query::default(),
        "ECOSYSTEM_NAME",
        &mut accumulator,
    ))
    .unwrap_err();

    assert!(matches!(error, EngineError::Format(_)), "{error:?}");
    assert!(
        format!("{error}").contains("ECOSYSTEM_NAME"),
        "the message should name the column: {error}"
    );
}

#[test]
fn bytes_that_are_not_parquet_are_reported_clearly() {
    let source = InMemorySource::new(b"<!DOCTYPE html><html>404 Not Found</html>".to_vec());
    let mut accumulator = DistributionAccumulator::new();

    let error = block_on(accumulate_geoparquet(
        &source,
        &Query::default(),
        ECO_COLUMN,
        &mut accumulator,
    ))
    .unwrap_err();

    assert!(matches!(error, EngineError::Format(_)), "{error:?}");
}

#[test]
fn the_report_accounts_for_every_byte_it_fetched() {
    // The report is what a caller uses to know whether pruning worked, so it has to
    // agree with what actually crossed the wire rather than being an estimate.
    let source = Watched::new(fixture("ecosystems.parquet"));
    let mut accumulator = DistributionAccumulator::new();

    let report = block_on(accumulate_geoparquet(
        &source,
        &Query::default(),
        ECO_COLUMN,
        &mut accumulator,
    ))
    .unwrap();

    assert_eq!(report.bytes_fetched, source.bytes_fetched());
}
