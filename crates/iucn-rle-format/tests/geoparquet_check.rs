//! Structural checks on a file's metadata, beyond what a JSON Schema can see.
//!
//! The published `GeoParquet` schemas validate that the `geo` blob is well-formed JSON
//! of the right shape. They cannot check it against the file it describes — whether
//! `primary_column` names a column that exists, whether the covering columns are
//! really there, whether the coordinates are in a CRS the metrics can use. Every bug
//! real data has exposed in this reader so far lived in that gap: those files were all
//! schema-valid.
//!
//! Findings are advisory by design. A technically imperfect file is usually still
//! readable, and refusing to read one would block real work for no gain.

// Byte offsets narrowed to index a Vec; fixtures are kilobytes and hosts are 64-bit.
#![allow(clippy::cast_possible_truncation)]

use std::fs;
use std::path::PathBuf;

use iucn_rle_format::geoparquet::{
    footer_range, parse_footer, Footer, GeoParquet, Severity, DEFAULT_FOOTER_PREFETCH,
};

fn bytes_of(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/data")
        .join(name);
    fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn open_bytes(bytes: &[u8]) -> GeoParquet {
    let size = bytes.len() as u64;
    let range = footer_range(size, DEFAULT_FOOTER_PREFETCH);
    let Footer::Complete(file) =
        parse_footer(&bytes[range.start as usize..range.end as usize], size).unwrap()
    else {
        panic!("the fixture footer fits in one prefetch")
    };
    *file
}

fn open(name: &str) -> GeoParquet {
    open_bytes(&bytes_of(name))
}

/// Rewrite a byte string in place, keeping every offset in the footer intact.
///
/// A re-encode would move the thrift structures the footer points at; a same-length
/// substitution leaves them exactly where they were.
fn patch(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    assert_eq!(
        from.len(),
        to.len(),
        "the replacement must be the same length"
    );
    let mut out = bytes.to_vec();
    let position = out
        .windows(from.len())
        .position(|window| window == from.as_bytes())
        .unwrap_or_else(|| panic!("{from:?} is not in this fixture"));
    out[position..position + from.len()].copy_from_slice(to.as_bytes());
    out
}

fn messages(findings: &[iucn_rle_format::geoparquet::Finding]) -> String {
    findings
        .iter()
        .map(|finding| format!("{:?}: {}", finding.severity, finding.message))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_well_formed_file_reports_no_problems() {
    let findings = open("ecosystems.parquet").check_structure();

    assert!(
        !findings.iter().any(|f| f.severity == Severity::Error),
        "expected a clean bill of health, got:\n{}",
        messages(&findings)
    );
}

#[test]
fn a_geometry_column_that_does_not_exist_is_an_error() {
    // The check a JSON Schema cannot make: `primary_column` is a valid string, and it
    // names nothing. Without this the failure surfaces later as "no column named
    // geometrz", at the point of reading rather than the point of describing.
    let bytes = patch(
        &bytes_of("ecosystems.parquet"),
        r#"{"primary_column": "geometry", "columns": {"geometry""#,
        r#"{"primary_column": "geometrz", "columns": {"geometrz""#,
    );

    let findings = open_bytes(&bytes).check_structure();

    assert!(
        findings
            .iter()
            .any(|f| f.severity == Severity::Error && f.message.contains("geometrz")),
        "expected an error naming the missing column, got:\n{}",
        messages(&findings)
    );
}

#[test]
fn covering_columns_that_do_not_exist_are_an_error() {
    // Worse than useless if unnoticed: the reader falls back to reading every row
    // group, so the file behaves correctly and every spatial query pays full price
    // with nothing to indicate why.
    let mut bytes = bytes_of("ecosystems.parquet");
    for axis in ["xmin", "ymin", "xmax", "ymax"] {
        bytes = patch(
            &bytes,
            &format!(r#"["bbox", "{axis}"]"#),
            &format!(r#"["bbxx", "{axis}"]"#),
        );
    }

    let findings = open_bytes(&bytes).check_structure();

    assert!(
        findings
            .iter()
            .any(|f| f.severity == Severity::Error && f.message.contains("bbxx")),
        "expected an error naming the missing covering column, got:\n{}",
        messages(&findings)
    );
}

#[test]
fn a_file_without_covering_is_noted_but_not_an_error() {
    // GeoParquet 1.0 files have none, including Colombia's national map. It is worth
    // saying — every spatial query must read everything — but it is not a defect.
    let findings = open("ecosystems_no_covering.parquet").check_structure();

    assert!(
        !findings.iter().any(|f| f.severity == Severity::Error),
        "absent covering is not an error:\n{}",
        messages(&findings)
    );
    assert!(
        findings
            .iter()
            .any(|f| f.severity == Severity::Note && f.message.contains("covering")),
        "it should still be mentioned:\n{}",
        messages(&findings)
    );
}

#[test]
fn a_projected_coordinate_system_is_an_error() {
    let findings = open("ecosystems_projected.parquet").check_structure();

    assert!(
        findings
            .iter()
            .any(|f| f.severity == Severity::Error && f.message.contains("3116")),
        "expected an error naming the projected CRS, got:\n{}",
        messages(&findings)
    );
}

#[test]
fn a_national_geographic_datum_is_accepted_without_complaint() {
    // MAGNA-SIRGAS is not EPSG:4326 and is perfectly usable. Flagging it would train
    // people to ignore the output.
    let findings = open("ecosystems_magna.parquet").check_structure();

    assert!(
        !findings.iter().any(|f| f.severity == Severity::Error),
        "EPSG:4686 is geographic and fine:\n{}",
        messages(&findings)
    );
}

#[test]
fn findings_are_ordered_with_the_most_serious_first() {
    // The output is meant to be read top-down and acted on; burying an error under
    // three notes defeats the purpose.
    let bytes = patch(
        &bytes_of("ecosystems_no_covering.parquet"),
        r#"{"primary_column": "geometry", "columns": {"geometry""#,
        r#"{"primary_column": "geometrz", "columns": {"geometrz""#,
    );

    let findings = open_bytes(&bytes).check_structure();

    assert!(findings.len() > 1, "this fixture should raise several");
    let severities: Vec<_> = findings.iter().map(|f| f.severity).collect();
    let mut sorted = severities.clone();
    sorted.sort();
    assert_eq!(severities, sorted, "not ordered:\n{}", messages(&findings));
    assert_eq!(findings[0].severity, Severity::Error);
}

#[test]
fn the_declared_version_is_reported_when_it_understates_the_file() {
    // geopandas writes "1.0.0" while using the `covering` key introduced in 1.1.0.
    // That is legal — the schema allows unknown keys — so schema validation passes and
    // says nothing. It is still worth knowing, because a stricter reader would drop
    // the covering and quietly lose all spatial pruning.
    let findings = open("ecosystems.parquet").check_structure();

    assert!(
        findings.iter().any(|f| {
            f.severity == Severity::Note && f.message.contains("1.0.0") && f.message.contains("1.1")
        }),
        "expected a note about the understated version, got:\n{}",
        messages(&findings)
    );
}
