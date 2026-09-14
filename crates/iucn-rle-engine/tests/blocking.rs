//! The synchronous entry point Python, R and the CLI call.
//!
//! Driven over an in-memory source rather than HTTP: what is being tested here is that
//! blocking on a fresh runtime produces the same answers as awaiting, and that the
//! metrics and the read report arrive together. The transport has its own tests, and
//! the binding tests exercise a real socket.

use std::fs;
use std::path::PathBuf;

use iucn_rle_engine::blocking::distribution_metrics_from_source;
use iucn_rle_engine::{EngineError, ReadOptions};
use iucn_rle_format::geoparquet::{Bbox, Query};
use iucn_rle_io::InMemorySource;

const ECO_COLUMN: &str = "eco_code";

fn source(name: &str) -> InMemorySource {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/data")
        .join(name);
    InMemorySource::new(
        fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display())),
    )
}

fn read(source: &InMemorySource, query: &Query) -> iucn_rle_engine::blocking::RemoteMetrics {
    distribution_metrics_from_source(source, ECO_COLUMN, query, &ReadOptions::default())
        .expect("the fixture reads")
}

#[test]
fn blocking_returns_the_metrics_and_what_they_cost() {
    let result = read(&source("ecosystems.parquet"), &Query::default());

    let codes: Vec<_> = result
        .metrics
        .ecosystems
        .iter()
        .map(|e| e.ecosystem.as_str())
        .collect();
    assert_eq!(codes, ["ECO_A", "ECO_B", "ECO_C", "ECO_D"]);
    assert_eq!(result.read.features, 16);
    assert_eq!(result.read.row_groups_read, 4);
    assert!(result.metrics.ecosystems[2].eoo_km2 > 0.0);
}

#[test]
fn a_filtered_read_gives_the_same_answer_for_what_it_kept() {
    // Pruning is an optimisation, so it must be invisible in the output. If these ever
    // disagree, the statistics are being trusted for something they do not prove.
    let file = source("ecosystems.parquet");
    let whole = read(&file, &Query::default());
    let filtered = read(&file, &Query::default().with_ecosystems(["ECO_B"]));

    let expected = whole
        .metrics
        .ecosystems
        .iter()
        .find(|e| e.ecosystem == "ECO_B")
        .expect("ECO_B is in the fixture");

    assert_eq!(
        filtered.metrics.ecosystems.as_slice(),
        std::slice::from_ref(expected)
    );
    assert_eq!(filtered.read.row_groups_skipped, 3);
}

#[test]
fn a_box_over_empty_ocean_finds_nothing() {
    let result = read(
        &source("ecosystems.parquet"),
        &Query::default().with_bbox(Bbox::new(-170.0, -60.0, -160.0, -50.0)),
    );

    assert!(result.metrics.ecosystems.is_empty());
    assert_eq!(result.read.features, 0);
}

#[test]
fn the_result_carries_the_same_provenance_as_a_local_computation() {
    // The grid CRS and cell size come out of the same place either way, so a caller
    // cannot tell from the output whether the polygons were handed in or streamed.
    let result = read(&source("ecosystems.parquet"), &Query::default());

    assert_eq!(result.metrics.grid_crs, "ESRI:54034");
    assert!((result.metrics.cell_size_m - 10_000.0).abs() < f64::EPSILON);
    assert!(result.read.crs.is_some(), "the file declares a CRS");
}

#[test]
fn a_mistyped_column_is_refused_before_any_data_is_fetched() {
    let error = distribution_metrics_from_source(
        &source("ecosystems.parquet"),
        "ECOSYSTEM",
        &Query::default(),
        &ReadOptions::default(),
    )
    .unwrap_err();

    // The message has to name what is available: this is the likeliest user error, and
    // "no such column" alone leaves the caller guessing.
    let message = error.to_string();
    assert!(matches!(error, EngineError::Format(_)), "{error:?}");
    assert!(message.contains("eco_code"), "{message}");
}

#[test]
fn the_json_shape_is_the_local_one_plus_a_read_key() {
    // Bindings depend on this: `distribution_metrics` and this function must return
    // dicts that differ only by the added `read` key, so downstream code takes either.
    let result = read(&source("ecosystems.parquet"), &Query::default());
    let json: serde_json::Value = serde_json::to_value(&result).expect("serialises");

    for key in [
        "ecosystems",
        "overfull_cells",
        "grid_crs",
        "cell_size_m",
        "read",
    ] {
        assert!(json.get(key).is_some(), "missing `{key}` in {json}");
    }
    assert!(json["read"]["bytes_fetched"].as_u64().unwrap() > 0);
}
