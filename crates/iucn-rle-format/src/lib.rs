//! Synchronous decoders for the formats an RLE assessment reads.
//!
//! Every decoder here takes `&[u8]` already in memory and returns plain data. There
//! is no async, no network, and no `Send` bound anywhere, which is what lets the whole
//! stack target `wasm32-unknown-unknown`. Fetching bytes is `iucn-rle-io`'s job.

pub mod geoparquet;
pub mod wkb;
