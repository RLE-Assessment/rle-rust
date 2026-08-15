//! Decoding Well-Known Binary geometry.
//!
//! WKB is how `GeoParquet` stores geometry, so every remote vector read passes through
//! here. The bytes below are hand-assembled rather than produced by a library, so the
//! tests describe the format itself rather than agreeing with whatever wrote them.
//!
//! Exact float comparison is deliberate throughout. Decoding does no arithmetic — it
//! copies eight bytes and reinterprets them — so a coordinate that differs from the one
//! written is a decoding bug, not rounding. A tolerance would hide exactly the
//! off-by-one-ordinate errors these tests exist to catch.
#![allow(clippy::float_cmp)]

use iucn_rle_format::wkb::{decode_polygons, WkbError};

/// Little-endian byte order marker (NDR).
const LE: u8 = 1;
/// Big-endian byte order marker (XDR).
const BE: u8 = 0;

fn le_u32(value: u32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn le_f64(value: f64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// A closed square ring, counter-clockwise, as WKB coordinates.
fn square_ring(min: f64, size: f64) -> Vec<u8> {
    let corners = [
        (min, min),
        (min + size, min),
        (min + size, min + size),
        (min, min + size),
        (min, min),
    ];
    let mut out = le_u32(u32::try_from(corners.len()).unwrap());
    for (x, y) in corners {
        out.extend(le_f64(x));
        out.extend(le_f64(y));
    }
    out
}

/// A little-endian WKB Polygon with one exterior ring.
fn polygon_wkb(min: f64, size: f64) -> Vec<u8> {
    let mut out = vec![LE];
    out.extend(le_u32(3)); // Polygon
    out.extend(le_u32(1)); // one ring
    out.extend(square_ring(min, size));
    out
}

#[test]
fn a_polygon_decodes_to_one_ring() {
    let polygons = decode_polygons(&polygon_wkb(0.0, 10.0)).unwrap();

    assert_eq!(polygons.len(), 1);
    assert_eq!(polygons[0].len(), 1, "one ring");
    assert_eq!(polygons[0][0].len(), 5, "five points, closed");
    assert_eq!(polygons[0][0][0], [0.0, 0.0]);
    assert_eq!(polygons[0][0][2], [10.0, 10.0]);
}

#[test]
fn a_polygon_with_a_hole_keeps_both_rings_in_order() {
    // Ring order carries meaning: the first is the exterior and the rest are holes.
    // Reordering them would turn a hole into a separate patch.
    let mut wkb = vec![LE];
    wkb.extend(le_u32(3));
    wkb.extend(le_u32(2)); // two rings
    wkb.extend(square_ring(0.0, 10.0));
    wkb.extend(square_ring(2.0, 3.0));

    let polygons = decode_polygons(&wkb).unwrap();

    assert_eq!(polygons[0].len(), 2);
    assert_eq!(polygons[0][0][0], [0.0, 0.0], "exterior first");
    assert_eq!(polygons[0][1][0], [2.0, 2.0], "hole second");
}

#[test]
fn a_multipolygon_yields_each_part_separately() {
    // Each part of a MultiPolygon is its own polygon for both metrics, so they must
    // not be flattened into one ring list.
    let part_a = polygon_wkb(0.0, 5.0);
    let part_b = polygon_wkb(100.0, 5.0);

    let mut wkb = vec![LE];
    wkb.extend(le_u32(6)); // MultiPolygon
    wkb.extend(le_u32(2)); // two parts
    wkb.extend(part_a);
    wkb.extend(part_b);

    let polygons = decode_polygons(&wkb).unwrap();

    assert_eq!(polygons.len(), 2);
    assert_eq!(polygons[0][0][0], [0.0, 0.0]);
    assert_eq!(polygons[1][0][0], [100.0, 100.0]);
}

#[test]
fn big_endian_geometry_decodes() {
    // XDR is rare but legal, and a file that used it would otherwise be read as
    // garbage coordinates rather than rejected.
    let mut wkb = vec![BE];
    wkb.extend(3u32.to_be_bytes());
    wkb.extend(1u32.to_be_bytes());
    wkb.extend(4u32.to_be_bytes());
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)] {
        wkb.extend(f64::to_be_bytes(x));
        wkb.extend(f64::to_be_bytes(y));
    }

    let polygons = decode_polygons(&wkb).unwrap();
    assert_eq!(polygons[0][0][2], [1.0, 1.0]);
}

#[test]
fn a_multipolygon_may_mix_byte_orders() {
    // Each part carries its own byte-order marker. Assuming the container's order
    // applies throughout is a classic WKB bug.
    let mut little = vec![LE];
    little.extend(le_u32(3));
    little.extend(le_u32(1));
    little.extend(square_ring(0.0, 5.0));

    let mut big = vec![BE];
    big.extend(3u32.to_be_bytes());
    big.extend(1u32.to_be_bytes());
    big.extend(4u32.to_be_bytes());
    for (x, y) in [(50.0, 50.0), (55.0, 50.0), (55.0, 55.0), (50.0, 50.0)] {
        big.extend(f64::to_be_bytes(x));
        big.extend(f64::to_be_bytes(y));
    }

    let mut wkb = vec![LE];
    wkb.extend(le_u32(6));
    wkb.extend(le_u32(2));
    wkb.extend(little);
    wkb.extend(big);

    let polygons = decode_polygons(&wkb).unwrap();
    assert_eq!(polygons.len(), 2);
    assert_eq!(polygons[1][0][0], [50.0, 50.0]);
}

#[test]
fn z_coordinates_are_skipped_not_misread() {
    // ISO WKB adds 1000 to the type code for Z. Reading a 3D polygon as 2D without
    // accounting for the extra ordinate shifts every coordinate after the first and
    // produces a plausible-looking but wrong shape.
    let mut wkb = vec![LE];
    wkb.extend(le_u32(1003)); // PolygonZ
    wkb.extend(le_u32(1));
    wkb.extend(le_u32(4));
    for (x, y, z) in [
        (0.0, 0.0, 100.0),
        (10.0, 0.0, 105.0),
        (10.0, 10.0, 110.0),
        (0.0, 0.0, 100.0),
    ] {
        wkb.extend(le_f64(x));
        wkb.extend(le_f64(y));
        wkb.extend(le_f64(z));
    }

    let polygons = decode_polygons(&wkb).unwrap();
    assert_eq!(polygons[0][0][1], [10.0, 0.0], "z must not shift x and y");
    assert_eq!(polygons[0][0][2], [10.0, 10.0]);
}

#[test]
fn measured_and_three_dimensional_geometry_decodes() {
    // 2000 adds M, 3000 adds both. Each drops the extra ordinates.
    for (type_code, extra) in [(2003u32, 1usize), (3003, 2)] {
        let mut wkb = vec![LE];
        wkb.extend(le_u32(type_code));
        wkb.extend(le_u32(1));
        wkb.extend(le_u32(3));
        for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 0.0)] {
            wkb.extend(le_f64(x));
            wkb.extend(le_f64(y));
            for _ in 0..extra {
                wkb.extend(le_f64(999.0));
            }
        }

        let polygons = decode_polygons(&wkb).unwrap();
        assert_eq!(polygons[0][0][1], [1.0, 0.0], "type {type_code}");
    }
}

#[test]
fn postgis_ewkb_with_an_srid_decodes() {
    // PostGIS sets high bits instead of adding thousands, and inserts an SRID after
    // the type. GeoParquet written by PostGIS-derived tooling carries this.
    let mut wkb = vec![LE];
    wkb.extend(le_u32(0x2000_0003)); // Polygon with SRID flag
    wkb.extend(le_u32(4326)); // the SRID
    wkb.extend(le_u32(1));
    wkb.extend(square_ring(0.0, 2.0));

    let polygons = decode_polygons(&wkb).unwrap();
    assert_eq!(polygons[0][0][0], [0.0, 0.0]);
}

#[test]
fn a_non_areal_geometry_is_rejected_by_name() {
    // A Point or LineString in an ecosystem distribution is a data problem. Silently
    // returning no polygons would understate the AOO with no indication why.
    let mut wkb = vec![LE];
    wkb.extend(le_u32(1)); // Point
    wkb.extend(le_f64(1.0));
    wkb.extend(le_f64(2.0));

    let err = decode_polygons(&wkb).unwrap_err();
    assert!(
        matches!(err, WkbError::NotAreal { .. }),
        "expected NotAreal, got {err:?}"
    );
    assert!(format!("{err}").contains("Point"), "{err}");
}

#[test]
fn truncated_bytes_are_an_error_not_a_partial_shape() {
    // Half a coordinate list must not decode to a smaller valid-looking polygon.
    let full = polygon_wkb(0.0, 10.0);
    for cut in [1, 5, 9, 20, full.len() - 1] {
        let err = decode_polygons(&full[..cut]).unwrap_err();
        assert!(
            matches!(err, WkbError::Truncated { .. }),
            "cut at {cut} gave {err:?}"
        );
    }
}

#[test]
fn empty_bytes_are_an_error() {
    assert!(decode_polygons(&[]).is_err());
}

#[test]
fn an_unknown_byte_order_marker_is_rejected() {
    let err = decode_polygons(&[7, 0, 0, 0, 0]).unwrap_err();
    assert!(matches!(err, WkbError::BadByteOrder(7)), "{err:?}");
}

#[test]
fn an_absurd_ring_count_does_not_allocate_wildly() {
    // A corrupt length field claiming four billion rings must fail on the bytes not
    // being there, rather than attempting the allocation first.
    let mut wkb = vec![LE];
    wkb.extend(le_u32(3));
    wkb.extend(le_u32(u32::MAX));

    let err = decode_polygons(&wkb).unwrap_err();
    assert!(matches!(err, WkbError::Truncated { .. }), "{err:?}");
}
