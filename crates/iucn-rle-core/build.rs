//! Hashes the threshold table at build time.
//!
//! The digest is pinned into every assessment's provenance so a reviewer can
//! prove which edition of the Guidelines produced a category. Computing it here
//! rather than at runtime keeps `sha2` out of the shipped binary — the core-only
//! WASM bundle is a stated design goal, and it should not carry a hash
//! implementation to report a constant.

use std::path::Path;

use sha2::{Digest, Sha256};

const THRESHOLDS: &str = "thresholds/iucn-rle-v2.0-2024.toml";

fn main() {
    println!("cargo:rerun-if-changed={THRESHOLDS}");
    println!("cargo:rerun-if-changed=build.rs");

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(THRESHOLDS);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    println!(
        "cargo:rustc-env=IUCN_RLE_THRESHOLDS_V2_2024_SHA256={:x}",
        Sha256::digest(&bytes)
    );
}
