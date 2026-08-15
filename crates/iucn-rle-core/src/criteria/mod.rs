//! Criterion evaluation.
//!
//! Only Criterion B is implemented. A, C, D, and E arrive in later milestones;
//! see `thresholds/iucn-rle-v2.0-2024.toml` for what has and has not been
//! transcribed from the Guidelines.

pub mod b;

pub use b::criterion_b;
