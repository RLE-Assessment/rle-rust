//! Opening a remote `GeoParquet` file: how many bytes to ask for, and what they say.
//!
//! Everything here runs against `fixtures/data/ecosystems.parquet`, written by geopandas
//! — so these tests check agreement with a real writer rather than with the Rust side's
//! own assumptions. `tools/generate_geoparquet_fixture.py --check` keeps it current.

// Byte offsets are u64 because a remote object can exceed 4 GB, but these fixtures are
// a few kilobytes and the tests run on 64-bit hosts, so narrowing them to index a Vec is
// exact here.
#![allow(clippy::cast_possible_truncation)]

use std::fs;
use std::path::PathBuf;

use iucn_rle_format::geoparquet::{
    footer_range, parse_footer, Footer, FormatError, DEFAULT_FOOTER_PREFETCH,
};

fn fixture() -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/data/ecosystems.parquet");
    fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}. Regenerate it with \
             `python3 tools/generate_geoparquet_fixture.py`",
            path.display()
        )
    })
}

/// Open the fixture the way a remote reader would: one suffix fetch, then parse.
fn open_fixture() -> iucn_rle_format::geoparquet::GeoParquet {
    let bytes = fixture();
    let size = bytes.len() as u64;
    let range = footer_range(size, DEFAULT_FOOTER_PREFETCH);
    let tail = &bytes[range.start as usize..range.end as usize];
    match parse_footer(tail, size).unwrap() {
        Footer::Complete(file) => *file,
        Footer::NeedMore(_) => panic!("the fixture footer should fit in one prefetch"),
    }
}

#[test]
fn the_first_fetch_asks_for_the_end_of_the_file() {
    // The footer is at the end and its length is in the last eight bytes, so a reader
    // that guesses generously reads the whole footer in one request instead of two.
    let range = footer_range(10_000_000, DEFAULT_FOOTER_PREFETCH);

    assert_eq!(range.end, 10_000_000, "must reach the last byte");
    assert_eq!(range.start, 10_000_000 - DEFAULT_FOOTER_PREFETCH);
}

#[test]
fn a_file_smaller_than_the_prefetch_is_read_whole() {
    // Asking for bytes before the start of the object is a malformed request that a
    // real HTTP server would reject outright.
    let range = footer_range(500, DEFAULT_FOOTER_PREFETCH);
    assert_eq!(range.start, 0);
    assert_eq!(range.end, 500);
}

#[test]
fn a_footer_that_fits_in_the_prefetch_parses_in_one_request() {
    let file = open_fixture();
    assert_eq!(file.num_rows(), 16);
    assert_eq!(file.row_groups().len(), 4);
}

#[test]
fn a_footer_larger_than_the_prefetch_asks_for_a_second_range_that_works() {
    // The two-request path, and the assertion that matters is not that it reports
    // NeedMore but that the range it names actually succeeds. An off-by-one here would
    // send a reader into an infinite fetch loop.
    let bytes = fixture();
    let size = bytes.len() as u64;

    // Deliberately too small to contain the footer, but enough for the length trailer.
    let stingy = footer_range(size, 64);
    let tail = &bytes[stingy.start as usize..stingy.end as usize];

    let Footer::NeedMore(wider) = parse_footer(tail, size).unwrap() else {
        panic!("a 64-byte prefetch should not be enough for this footer");
    };
    assert_eq!(wider.end, size, "the second range must still reach the end");
    assert!(wider.start < stingy.start, "it must ask for more, not less");

    let retry = &bytes[wider.start as usize..wider.end as usize];
    assert!(
        matches!(parse_footer(retry, size), Ok(Footer::Complete(_))),
        "the range NeedMore asked for must be sufficient"
    );
}

#[test]
fn a_file_too_short_to_hold_a_footer_is_rejected() {
    let err = parse_footer(b"PAR1", 4).unwrap_err();
    assert!(matches!(err, FormatError::NotParquet { .. }), "{err:?}");
}

#[test]
fn a_file_without_the_parquet_magic_is_rejected_by_name() {
    // The common cause is a URL that returns an error page, a redirect body, or HTML.
    // Saying so beats a thrift decoding error from deep inside the parquet crate.
    let err = parse_footer(b"<!DOCTYPE html><html>oops</html>", 32).unwrap_err();
    assert!(matches!(err, FormatError::NotParquet { .. }), "{err:?}");
}

#[test]
fn an_encrypted_file_is_reported_as_encrypted() {
    // Modular encryption writes PARE instead of PAR1. Without this the reader would
    // report a corrupt footer and send the user looking for a damaged file.
    let mut bytes = vec![0u8; 8];
    bytes[4..].copy_from_slice(b"PARE");
    let err = parse_footer(&bytes, 8).unwrap_err();
    assert!(matches!(err, FormatError::Encrypted), "{err:?}");
}

#[test]
fn the_primary_geometry_column_is_read_from_the_geo_metadata() {
    // A GeoParquet file may carry several geometry columns; `primary_column` names the
    // one to use. Assuming it is called "geometry" happens to work here and fails on
    // files written from PostGIS, where it is often "geom" or "wkb_geometry".
    let file = open_fixture();
    assert_eq!(file.geo().primary_column, "geometry");
}

#[test]
fn wkb_encoding_is_recognised() {
    let file = open_fixture();
    assert!(
        file.geo().primary_is_wkb(),
        "geopandas writes WKB; encoding was {:?}",
        file.geo().primary_encoding()
    );
}

#[test]
fn the_declared_crs_is_available() {
    // Coordinates are meaningless without it: the AOO grid is defined in ESRI:54034 and
    // reprojection has to start from something known.
    let file = open_fixture();
    assert_eq!(file.geo().primary_crs_code(), Some("EPSG:4326".to_owned()));
}

#[test]
fn bbox_covering_columns_are_found_even_though_the_declared_version_is_1_0_0() {
    // The load-bearing real-world quirk, and the reason this fixture is generated by
    // geopandas rather than hand-written. geopandas 1.1.4 writes `"version": "1.0.0"`
    // while also writing the `covering` key that only exists in GeoParquet 1.1. A
    // reader that gates covering support on the declared version silently loses all
    // bbox pruning on files from the most widely used writer there is.
    let file = open_fixture();
    assert_eq!(
        file.geo().version,
        "1.0.0",
        "the fixture's declared version"
    );

    let covering = file
        .geo()
        .primary_covering()
        .expect("covering must be found regardless of declared version");
    assert_eq!(covering.xmin, ["bbox", "xmin"]);
    assert_eq!(covering.ymin, ["bbox", "ymin"]);
    assert_eq!(covering.xmax, ["bbox", "xmax"]);
    assert_eq!(covering.ymax, ["bbox", "ymax"]);
}

#[test]
fn a_file_with_no_geo_metadata_is_rejected_as_not_geoparquet() {
    // Plain parquet is a legitimate file that simply is not what was asked for. The
    // error should say which, rather than reporting a missing column later on.
    let bytes = fixture();
    let stripped = strip_geo_metadata(&bytes);
    let size = stripped.len() as u64;
    let range = footer_range(size, DEFAULT_FOOTER_PREFETCH);

    let err = parse_footer(&stripped[range.start as usize..range.end as usize], size).unwrap_err();
    assert!(matches!(err, FormatError::NotGeoParquet), "{err:?}");
}

/// Blank out the `geo` key so the file stays valid parquet but stops being `GeoParquet`.
///
/// Renaming the key in place keeps every offset in the footer intact, which a
/// re-encode would not.
fn strip_geo_metadata(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    // The key is exactly "geo", so a plain search would hit "geometry" inside the
    // pandas metadata block first and leave the real key untouched. Require that the
    // next byte is not part of a longer word.
    let position = out
        .windows(4)
        .position(|window| {
            &window[..3] == b"geo" && !window[3].is_ascii_alphanumeric() && window[3] != b'_'
        })
        .expect("the fixture has a geo metadata key");
    out[position..position + 3].copy_from_slice(b"zzz");
    out
}
