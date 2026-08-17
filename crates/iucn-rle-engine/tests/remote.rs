//! Reading a real published dataset over the network.
//!
//! Every other test in this workspace runs against fixtures, which is what keeps them
//! fast and offline. This one exists because fixtures cannot tell you whether the thing
//! works on data somebody actually published — and each time it has been pointed at
//! real data it has found something no fixture did: a HEAD response whose body length
//! is zero, a national CRS that is geographic but not EPSG:4326, ZSTD compression
//! compiled out of the build.
//!
//! **These tests are `#[ignore]`d** because they need the network and a third party's
//! server. Run them deliberately:
//!
//! ```text
//! cargo test -p iucn-rle-engine --test remote -- --ignored --nocapture
//! ```
//!
//! The target is the Bogotá-area subset (16 MB), not the national file (1.8 GB). A test
//! that pulled nearly two gigabytes from someone else's bucket on every run would be a
//! poor neighbour, and it would prove nothing the subset does not. The national file is
//! exercised by hand through the `assess` example, and its numbers are recorded in the
//! commit that introduced it.

#![allow(clippy::cast_possible_truncation)]

use iucn_rle_core::distribution::DistributionAccumulator;
use iucn_rle_engine::accumulate_geoparquet;
use iucn_rle_format::geoparquet::Query;
use iucn_rle_io::HttpSource;

/// Colombia's national ecosystems map, clipped to the capital region.
const BOGOTA: &str = "https://data.source.coop/tyler/colombia-ecosystems-map/\
                      ecosistemas/ECOSISTEMAS_MEC_122024_bogota_area.parquet";

/// The ecosystem classification column, as named in that dataset.
const ECOSYSTEM_COLUMN: &str = "ecos_general";

/// Its size, so the test can say what fraction was fetched.
const FILE_SIZE: u64 = 16_390_311;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime")
        .block_on(future)
}

#[test]
#[ignore = "needs the network"]
fn a_published_dataset_reads_end_to_end() {
    let source = HttpSource::new(BOGOTA).expect("a usable URL");
    let mut accumulator = DistributionAccumulator::new();

    let report = block_on(accumulate_geoparquet(
        &source,
        &Query::default(),
        ECOSYSTEM_COLUMN,
        &mut accumulator,
    ))
    .expect("the dataset should read");

    assert_eq!(report.features, 4_944, "every feature in the subset");
    assert_eq!(
        report.crs.as_deref(),
        Some("EPSG:4686"),
        "MAGNA-SIRGAS, Colombia's national datum — geographic, but not EPSG:4326, \
         which an allow-list of one code would have refused"
    );

    let distribution = accumulator.finish();
    assert_eq!(distribution.ecosystems().len(), 34);

    // Metrics have to be real numbers. A read that fetched nothing would still report
    // success and leave an accumulator full of zeroes.
    for ecosystem in distribution.ecosystems() {
        assert!(
            distribution.eoo_km2(ecosystem) > 0.0,
            "{ecosystem} has no extent"
        );
        assert!(
            distribution.aoo(ecosystem).aoo_cells > 0,
            "{ecosystem} occupies no grid cell"
        );
    }
}

#[test]
#[ignore = "needs the network"]
fn only_the_columns_an_assessment_needs_are_fetched() {
    // The dataset has 50 columns and the assessment reads two. Fetching the rest would
    // give identical answers, so the saving is invisible unless it is asserted.
    let source = HttpSource::new(BOGOTA).expect("a usable URL");
    let mut accumulator = DistributionAccumulator::new();

    let report = block_on(accumulate_geoparquet(
        &source,
        &Query::default(),
        ECOSYSTEM_COLUMN,
        &mut accumulator,
    ))
    .expect("the dataset should read");

    assert!(
        report.bytes_fetched < FILE_SIZE,
        "fetched {} of {FILE_SIZE} bytes — column projection did nothing",
        report.bytes_fetched
    );
}

#[test]
#[ignore = "needs the network"]
fn a_missing_column_fails_before_any_geometry_is_fetched() {
    // On the national file the alternative is a gigabyte-and-a-half download that ends
    // in this same error.
    let source = HttpSource::new(BOGOTA).expect("a usable URL");
    let mut accumulator = DistributionAccumulator::new();

    let error = block_on(accumulate_geoparquet(
        &source,
        &Query::default(),
        "ecosistema",
        &mut accumulator,
    ))
    .expect_err("a column that is not there");

    let message = format!("{error}");
    assert!(message.contains("ecosistema"), "{message}");
    assert!(
        message.contains(ECOSYSTEM_COLUMN),
        "the message should list the columns that do exist: {message}"
    );
}

#[test]
#[ignore = "needs the network"]
fn the_web_ui_url_is_reported_as_not_being_parquet() {
    // source.coop serves its web interface at the same path shape as the data host, so
    // pasting the wrong one returns an HTML page with a 200 status. Without a specific
    // error this surfaces as a thrift decoding failure and reads as a corrupt file.
    let html = BOGOTA.replace("data.source.coop", "source.coop");
    let source = HttpSource::new(&html).expect("a usable URL");
    let mut accumulator = DistributionAccumulator::new();

    let error = block_on(accumulate_geoparquet(
        &source,
        &Query::default(),
        ECOSYSTEM_COLUMN,
        &mut accumulator,
    ))
    .expect_err("an HTML page is not a dataset");

    let message = format!("{error}");
    assert!(
        message.contains("parquet") || message.contains("range"),
        "the error should say what is wrong with the URL: {message}"
    );
}
