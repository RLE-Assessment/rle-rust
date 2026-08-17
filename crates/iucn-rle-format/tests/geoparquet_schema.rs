//! Validating the `geo` metadata against the schema published for its version.
//!
//! Complements the structural checks rather than replacing them. This says "the
//! metadata is well-formed per the specification"; the structural checks say "and it
//! describes the file it is actually in". Colombia's national map passes this and
//! failed the other, which is the whole reason both exist.
//!
//! Requires the `schema-validation` feature, and the schemas vendored by
//! `tools/vendor_geoparquet_schemas.py`. Validation never touches the network.

#![cfg(feature = "schema-validation")]
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

/// Same-length substitution, so every offset the footer points at stays put.
fn patch(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    assert_eq!(from.len(), to.len(), "replacement must be the same length");
    let mut out = bytes.to_vec();
    let position = out
        .windows(from.len())
        .position(|window| window == from.as_bytes())
        .unwrap_or_else(|| panic!("{from:?} is not in this fixture"));
    out[position..position + from.len()].copy_from_slice(to.as_bytes());
    out
}

fn describe(findings: &[iucn_rle_format::geoparquet::Finding]) -> String {
    findings
        .iter()
        .map(|f| format!("{:?}: {}", f.severity, f.message))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_conformant_file_validates() {
    let findings = open_bytes(&bytes_of("ecosystems.parquet")).validate_against_schema();

    assert!(
        findings.is_empty(),
        "expected no findings, got:\n{}",
        describe(&findings)
    );
}

#[test]
fn a_file_on_a_national_datum_validates() {
    // EPSG:4686 is a perfectly ordinary GeographicCRS, and the PROJJSON subschema
    // should accept it as readily as WGS84.
    let findings = open_bytes(&bytes_of("ecosystems_magna.parquet")).validate_against_schema();

    assert!(findings.is_empty(), "got:\n{}", describe(&findings));
}

#[test]
fn an_illegal_encoding_is_rejected() {
    // The schema constrains `encoding` to WKB or a native geometry type. "TWK" is
    // neither, and no amount of structural checking would notice the difference.
    let bytes = patch(
        &bytes_of("ecosystems.parquet"),
        r#""encoding": "WKB""#,
        r#""encoding": "TWK""#,
    );

    let findings = open_bytes(&bytes).validate_against_schema();

    assert!(
        findings.iter().any(|f| f.severity == Severity::Error),
        "expected a validation error, got:\n{}",
        describe(&findings)
    );
}

#[test]
fn the_projjson_reference_is_resolved_from_the_vendored_copy() {
    // The load-bearing test for offline validation. The `crs` field is a `$ref` to
    // PROJJSON on proj.org, and a validator that cannot resolve it either fails
    // outright or silently skips the subschema — the second being much worse, since
    // everything would appear to pass.
    //
    // Breaking the CRS *inside* that referenced schema's rules is the way to tell:
    // this is caught only if the reference was actually followed, and it is followed
    // from disk, since nothing here has network access.
    let bytes = patch(
        &bytes_of("ecosystems.parquet"),
        r#""type": "GeographicCRS""#,
        r#""type": "GeographicCRZ""#,
    );

    let findings = open_bytes(&bytes).validate_against_schema();

    assert!(
        findings.iter().any(|f| f.severity == Severity::Error),
        "a CRS that violates PROJJSON must be caught, which only happens if the \
         reference resolved; got:\n{}",
        describe(&findings)
    );
}

#[test]
fn a_version_with_no_published_schema_is_reported_rather_than_ignored() {
    // Each release's schema pins its own version with `const`, so validation must pick
    // the schema by what the file declares. A version nobody published cannot be
    // validated at all, and saying so is better than reporting a pass.
    let bytes = patch(
        &bytes_of("ecosystems.parquet"),
        r#""version": "1.0.0""#,
        r#""version": "9.9.9""#,
    );

    let findings = open_bytes(&bytes).validate_against_schema();

    assert!(
        findings
            .iter()
            .any(|f| f.severity == Severity::Warning && f.message.contains("9.9.9")),
        "expected a warning naming the unknown version, got:\n{}",
        describe(&findings)
    );
}

#[test]
fn every_published_version_has_a_usable_vendored_schema() {
    // Guards the vendoring: a schema that fails to compile would make validation
    // silently unavailable for files declaring that version.
    for version in iucn_rle_format::geoparquet::published_schema_versions() {
        assert!(
            iucn_rle_format::geoparquet::schema_for_version(version).is_some(),
            "no vendored schema compiles for {version}"
        );
    }
}
