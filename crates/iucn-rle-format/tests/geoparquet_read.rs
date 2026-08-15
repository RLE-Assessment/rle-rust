//! Reading only the parts of a `GeoParquet` file an assessment needs.
//!
//! This is the whole point of the format. A national dataset is gigabytes; an
//! assessment of one ecosystem touches a small fraction of it. These tests assert on
//! the **plan** — which row groups and which byte ranges — as well as on the decoded
//! results, because a reader that returns the right answer after downloading everything
//! is still the bug this milestone exists to fix.
#![allow(clippy::float_cmp)]

// Byte offsets are u64 because a remote object can exceed 4 GB, but these fixtures are
// a few kilobytes and the tests run on 64-bit hosts, so narrowing them to index a Vec is
// exact here.
#![allow(clippy::cast_possible_truncation)]

use std::fs;
use std::path::PathBuf;

use bytes::Bytes;
use iucn_rle_format::geoparquet::{
    footer_range, parse_footer, Bbox, Footer, FormatError, GeoParquet, Query, SparseBytes,
    DEFAULT_FOOTER_PREFETCH,
};

const ECO_COLUMN: &str = "eco_code";

fn read(name: &str) -> Vec<u8> {
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

fn open(name: &str) -> (GeoParquet, Vec<u8>) {
    let bytes = read(name);
    let size = bytes.len() as u64;
    let range = footer_range(size, DEFAULT_FOOTER_PREFETCH);
    let Footer::Complete(file) =
        parse_footer(&bytes[range.start as usize..range.end as usize], size).unwrap()
    else {
        panic!("the fixture footer fits in one prefetch")
    };
    (*file, bytes)
}

fn fixture() -> (GeoParquet, Vec<u8>) {
    open("ecosystems.parquet")
}

/// Serve a plan's byte ranges out of the whole file, as a real fetch would.
fn fetch(bytes: &[u8], plan: &iucn_rle_format::geoparquet::ReadPlan) -> SparseBytes {
    let chunks = plan
        .ranges()
        .map(|range| {
            (
                range.start,
                Bytes::copy_from_slice(&bytes[range.start as usize..range.end as usize]),
            )
        })
        .collect();
    SparseBytes::new(bytes.len() as u64, chunks)
}

#[test]
fn with_no_filter_every_row_group_is_selected() {
    let (file, _) = fixture();
    let plan = file.plan(&Query::default(), ECO_COLUMN).unwrap();

    assert_eq!(plan.row_groups(), &[0, 1, 2, 3]);
}

#[test]
fn a_bounding_box_selects_only_the_row_groups_it_touches() {
    // Each ecosystem sits in its own longitude band, so this is unambiguous: a box over
    // ECO_C's band must leave the other three row groups unfetched.
    let (file, _) = fixture();
    let query = Query::default().with_bbox(Bbox::new(-1.0, -1.0, 10.0, 5.0));

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    assert_eq!(plan.row_groups(), &[2], "only ECO_C's band overlaps");
}

#[test]
fn a_bounding_box_spanning_two_bands_selects_both() {
    let (file, _) = fixture();
    let query = Query::default().with_bbox(Bbox::new(-61.0, -1.0, 10.0, 5.0));

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    assert_eq!(plan.row_groups(), &[1, 2]);
}

#[test]
fn a_bounding_box_touching_nothing_selects_no_row_groups() {
    // The empty plan must be a legitimate result, not an error: a valid query over a
    // region an ecosystem does not reach is an ordinary thing to ask.
    let (file, _) = fixture();
    let query = Query::default().with_bbox(Bbox::new(100.0, 60.0, 120.0, 70.0));

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    assert!(plan.row_groups().is_empty());
    assert_eq!(plan.ranges().count(), 0, "nothing to fetch");
}

#[test]
fn a_box_sharing_only_an_edge_is_treated_as_overlapping() {
    // Touching is overlapping. Excluding an edge-touching row group would silently drop
    // features that lie exactly on it, which at a grid boundary is the common case.
    let (file, _) = fixture();
    let query = Query::default().with_bbox(Bbox::new(9.5, 0.0, 20.0, 4.0));

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    assert_eq!(plan.row_groups(), &[2], "x = 9.5 is ECO_C's exact maximum");
}

#[test]
fn an_ecosystem_filter_selects_only_matching_row_groups() {
    // Pruning on a string column's min/max statistics. This is what makes a
    // single-ecosystem assessment cheap on a national file of many ecosystems.
    let (file, _) = fixture();
    let query = Query::default().with_ecosystems(["ECO_B"]);

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    assert_eq!(plan.row_groups(), &[1]);
}

#[test]
fn several_ecosystems_select_several_row_groups() {
    let (file, _) = fixture();
    let query = Query::default().with_ecosystems(["ECO_A", "ECO_D"]);

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    assert_eq!(plan.row_groups(), &[0, 3]);
}

#[test]
fn an_ecosystem_that_is_absent_selects_nothing() {
    let (file, _) = fixture();
    let query = Query::default().with_ecosystems(["ECO_NOT_PRESENT"]);

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    assert!(plan.row_groups().is_empty());
}

#[test]
fn box_and_ecosystem_filters_both_apply() {
    // Both must hold. Taking the union rather than the intersection would read three
    // row groups where one is needed.
    let (file, _) = fixture();
    let query = Query::default()
        .with_bbox(Bbox::new(-100.0, -10.0, 10.0, 10.0))
        .with_ecosystems(["ECO_A"]);

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    assert_eq!(plan.row_groups(), &[0]);
}

#[test]
fn the_plan_fetches_far_less_than_the_whole_file() {
    // The claim this milestone rests on. One row group of two columns, out of four row
    // groups of four columns.
    let (file, bytes) = fixture();
    let query = Query::default().with_ecosystems(["ECO_B"]);

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    let fetched: u64 = plan.ranges().map(|range| range.end - range.start).sum();

    assert!(
        fetched * 4 < bytes.len() as u64,
        "fetched {fetched} of {} bytes, which is not a saving worth the complexity",
        bytes.len()
    );
}

#[test]
fn the_plan_does_not_fetch_columns_it_was_not_asked_for() {
    // feature_id is in the file and irrelevant to an assessment. Column pruning is half
    // the saving, and it is invisible unless asserted: reading it would still produce
    // correct answers.
    let (file, bytes) = fixture();
    let plan = file.plan(&Query::default(), ECO_COLUMN).unwrap();

    let fetched: Vec<u8> = plan
        .ranges()
        .flat_map(|range| bytes[range.start as usize..range.end as usize].to_vec())
        .collect();

    assert!(
        !contains(&fetched, b"ECO_A-0"),
        "the feature_id column was fetched but is never read"
    );
    assert!(
        contains(&fetched, b"ECO_A"),
        "the ecosystem column should have been fetched"
    );
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn a_selected_row_group_decodes_to_its_features() {
    let (file, bytes) = fixture();
    let query = Query::default().with_ecosystems(["ECO_C"]);
    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    let fetched = fetch(&bytes, &plan);

    let features = file.decode_row_group(2, &fetched, ECO_COLUMN).unwrap();

    assert_eq!(features.len(), 4);
    assert!(features.iter().all(|feature| feature.ecosystem == "ECO_C"));
}

#[test]
fn decoded_geometry_has_the_coordinates_that_were_written() {
    // The first ECO_C feature is a unit square with its lower-left corner at (0, 0).
    let (file, bytes) = fixture();
    let plan = file.plan(&Query::default(), ECO_COLUMN).unwrap();
    let features = file
        .decode_row_group(2, &fetch(&bytes, &plan), ECO_COLUMN)
        .unwrap();

    let first = &features[0];
    assert_eq!(first.polygons.len(), 1);
    assert_eq!(first.polygons[0][0][0], [0.0, 0.0]);
    assert_eq!(first.polygons[0][0].len(), 5, "closed ring");
}

#[test]
fn a_multipolygon_with_a_hole_survives_the_round_trip() {
    // The last feature of each ecosystem is a MultiPolygon whose first part has a hole.
    // Both are places a reader quietly goes wrong — flattening the parts, or promoting
    // the hole to a patch — and both would inflate the reported area.
    let (file, bytes) = fixture();
    let plan = file.plan(&Query::default(), ECO_COLUMN).unwrap();
    let features = file
        .decode_row_group(0, &fetch(&bytes, &plan), ECO_COLUMN)
        .unwrap();

    let last = features.last().unwrap();
    assert_eq!(last.polygons.len(), 2, "two parts, kept separate");
    assert_eq!(last.polygons[0].len(), 2, "exterior plus one hole");
    assert_eq!(last.polygons[1].len(), 1, "second part is solid");
}

#[test]
fn every_row_group_can_be_decoded() {
    let (file, bytes) = fixture();
    let plan = file.plan(&Query::default(), ECO_COLUMN).unwrap();
    let fetched = fetch(&bytes, &plan);

    let total: usize = plan
        .row_groups()
        .iter()
        .map(|&index| {
            file.decode_row_group(index, &fetched, ECO_COLUMN)
                .unwrap()
                .len()
        })
        .sum();

    assert_eq!(total, 16, "every row in the file");
}

#[test]
fn a_missing_ecosystem_column_is_reported_by_name() {
    // The likeliest user error: national datasets spell this column differently in
    // every country. The message has to say what was asked for.
    let (file, _) = fixture();
    let err = file.plan(&Query::default(), "ECOSYSTEM").unwrap_err();

    assert!(matches!(err, FormatError::NoSuchColumn { .. }), "{err:?}");
    assert!(format!("{err}").contains("ECOSYSTEM"), "{err}");
    assert!(
        format!("{err}").contains("eco_code"),
        "it should list what is available: {err}"
    );
}

#[test]
fn a_file_without_covering_columns_falls_back_to_reading_everything() {
    // GeoParquet 1.0 files have no bbox columns, so spatial pruning is impossible.
    // Returning nothing would be catastrophic and plausible-looking: an assessment that
    // silently found no ecosystem anywhere. Keep every row group instead.
    let (file, _) = open("ecosystems_no_covering.parquet");
    assert!(
        file.geo().primary_covering().is_none(),
        "this fixture is written without covering"
    );

    let query = Query::default().with_bbox(Bbox::new(-1.0, -1.0, 10.0, 5.0));
    let plan = file.plan(&query, ECO_COLUMN).unwrap();

    assert_eq!(
        plan.row_groups(),
        &[0, 1, 2, 3],
        "pruning must be conservative when it cannot prove absence"
    );
}

#[test]
fn a_file_without_covering_still_prunes_by_ecosystem() {
    // Losing spatial pruning must not lose the other kind: the ecosystem column's
    // statistics are ordinary parquet and are still there.
    let (file, _) = open("ecosystems_no_covering.parquet");
    let query = Query::default().with_ecosystems(["ECO_D"]);

    let plan = file.plan(&query, ECO_COLUMN).unwrap();
    assert_eq!(plan.row_groups(), &[3]);
}
