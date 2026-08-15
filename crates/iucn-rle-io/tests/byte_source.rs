//! The byte-range abstraction every remote format reads through.
//!
//! Cloud-optimised formats exist so a client can fetch the few kilobytes it needs
//! instead of the whole file. That requires exactly one capability — read a byte
//! range — and this is the trait for it.

use futures::executor::block_on;
use iucn_rle_io::{ByteSource, InMemorySource, IoError};

fn source() -> InMemorySource {
    InMemorySource::new(b"0123456789abcdef".to_vec())
}

#[test]
fn size_is_the_length_of_the_object() {
    assert_eq!(block_on(source().size()).unwrap(), 16);
}

#[test]
fn a_range_returns_exactly_those_bytes() {
    let bytes = block_on(source().read_range(4..8)).unwrap();
    assert_eq!(&bytes[..], b"4567");
}

#[test]
fn a_range_to_the_end_is_allowed() {
    let bytes = block_on(source().read_range(12..16)).unwrap();
    assert_eq!(&bytes[..], b"cdef");
}

#[test]
fn an_empty_range_returns_nothing() {
    let bytes = block_on(source().read_range(5..5)).unwrap();
    assert!(bytes.is_empty());
}

#[test]
fn reading_past_the_end_is_an_error_not_a_short_read() {
    // A short read would be indistinguishable from a truncated file, and a parser
    // would then report corrupt data rather than a bad request. Fail here instead.
    let err = block_on(source().read_range(12..99)).unwrap_err();
    assert!(
        matches!(err, IoError::OutOfBounds { .. }),
        "expected OutOfBounds, got {err:?}"
    );
}

#[test]
fn an_inverted_range_is_an_error() {
    // Built from variables rather than written as `8..4`, which clippy reads as a
    // typo for a reversed iterator. An inverted Range is exactly what is under test:
    // a parser computing `start..end` from a malformed header can produce one, and it
    // must be rejected rather than silently treated as empty.
    let (start, end) = (8u64, 4u64);
    let err = block_on(source().read_range(start..end)).unwrap_err();
    assert!(matches!(err, IoError::OutOfBounds { .. }), "got {err:?}");
}

#[test]
fn several_ranges_can_be_read_at_once() {
    // The shape that matters for a real fetch: a parser asks for a handful of
    // scattered extents, and the transport is free to issue them concurrently or
    // coalesce them into fewer requests.
    let ranges = [0..2, 6..8, 14..16];
    let parts = block_on(source().read_ranges(&ranges)).unwrap();

    assert_eq!(parts.len(), 3);
    assert_eq!(&parts[0][..], b"01");
    assert_eq!(&parts[1][..], b"67");
    assert_eq!(&parts[2][..], b"ef");
}

#[test]
fn reading_no_ranges_is_not_an_error() {
    let parts = block_on(source().read_ranges(&[])).unwrap();
    assert!(parts.is_empty());
}

#[test]
fn one_bad_range_fails_the_whole_batch() {
    // Partial success would leave a caller correlating results against requests to
    // discover which succeeded — an easy thing to get wrong quietly.
    let err = block_on(source().read_ranges(&[0..2, 99..100])).unwrap_err();
    assert!(matches!(err, IoError::OutOfBounds { .. }), "got {err:?}");
}

#[test]
fn the_suffix_of_a_file_can_be_read_without_knowing_its_length() {
    // How every parquet reader starts: the footer is at the end, and its length is
    // in the last eight bytes. One request rather than a HEAD followed by a GET.
    let bytes = block_on(source().read_suffix(4)).unwrap();
    assert_eq!(&bytes[..], b"cdef");
}

#[test]
fn a_suffix_longer_than_the_object_returns_the_whole_object() {
    let bytes = block_on(source().read_suffix(1_000)).unwrap();
    assert_eq!(bytes.len(), 16);
}

#[test]
fn an_empty_source_is_usable() {
    let empty = InMemorySource::new(Vec::new());
    assert_eq!(block_on(empty.size()).unwrap(), 0);
    assert!(block_on(empty.read_suffix(8)).unwrap().is_empty());
}
