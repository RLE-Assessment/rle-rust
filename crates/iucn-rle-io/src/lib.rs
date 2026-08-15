//! Byte-range readers for IUCN Red List of Ecosystems data sources.
//!
//! Cloud-optimised formats — `GeoParquet`, Cloud-Optimized `GeoTIFF` — exist so a
//! client can fetch the few kilobytes it needs rather than the whole file. That needs
//! exactly one capability: read a byte range. This crate is that, and nothing else.
//!
//! # The one place async lives
//!
//! Every future here is `?Send`, unconditionally. Browser `fetch` futures are not
//! `Send`, and requiring it would make the WebAssembly transport impossible to write.
//! Nothing is lost on native: I/O concurrency comes from `buffered()` on a
//! current-thread runtime, and CPU parallelism belongs to the decoders, not to the
//! async runtime.
//!
//! Everything above this crate parses **synchronously** over in-memory bytes. That is
//! what keeps `Send` bounds out of the parsers and lets the whole stack target
//! `wasm32-unknown-unknown`.
//!
//! # What is deliberately absent
//!
//! `object_store` is the obvious choice for this job and cannot be used: its `aws`,
//! `azure`, `gcp` and `http` features do not compile for `wasm32-unknown-unknown`,
//! because each bundles a native reqwest transport. `deny.toml` bans it outright so a
//! transitive dependency cannot quietly reintroduce it and break the browser build.

use core::ops::Range;

use bytes::Bytes;

#[cfg(feature = "http")]
mod http;
mod memory;

#[cfg(feature = "http")]
pub use http::HttpSource;
pub use memory::InMemorySource;

/// Something that went wrong fetching bytes.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    /// The requested range does not lie within the object.
    ///
    /// Deliberately an error rather than a short read: a truncated response is
    /// indistinguishable from a truncated file, and a parser given one would report
    /// corrupt data instead of a bad request.
    #[error("range {start}..{end} is outside an object of {size} bytes")]
    OutOfBounds {
        /// Start of the requested range.
        start: u64,
        /// End of the requested range.
        end: u64,
        /// Actual size of the object.
        size: u64,
    },

    /// The transport failed.
    #[error("{0}")]
    Transport(String),

    /// The server does not support range requests, so cloud-optimised access is
    /// impossible and the whole object would have to be downloaded.
    #[error(
        "{url} does not support HTTP range requests, so only whole-file access is \
         possible; check the server sends Accept-Ranges and, for browser use, exposes \
         Content-Range via CORS"
    )]
    RangesUnsupported {
        /// The URL that refused.
        url: String,
    },
}

/// A source of bytes addressable by range.
///
/// Implementations must be cheap to clone or share, since a parser will hold one for
/// the duration of a read.
#[async_trait::async_trait(?Send)]
pub trait ByteSource {
    /// Total size of the object in bytes.
    ///
    /// Implementations should cache this: it usually costs a request.
    async fn size(&self) -> Result<u64, IoError>;

    /// Read one byte range.
    ///
    /// # Errors
    ///
    /// [`IoError::OutOfBounds`] if the range is inverted or extends past the end of
    /// the object, and [`IoError::Transport`] if the fetch itself fails.
    async fn read_range(&self, range: Range<u64>) -> Result<Bytes, IoError>;

    /// Read several byte ranges.
    ///
    /// The default implementation reads them in sequence. Transports that can do
    /// better — issuing requests concurrently, or coalescing ranges that are close
    /// together into one request — should override it.
    ///
    /// One failed range fails the whole batch. Partial success would leave callers
    /// correlating results against requests to work out what they actually got, which
    /// is easy to get quietly wrong.
    ///
    /// # Errors
    ///
    /// As [`Self::read_range`].
    async fn read_ranges(&self, ranges: &[Range<u64>]) -> Result<Vec<Bytes>, IoError> {
        let mut parts = Vec::with_capacity(ranges.len());
        for range in ranges {
            parts.push(self.read_range(range.clone()).await?);
        }
        Ok(parts)
    }

    /// Read the last `length` bytes.
    ///
    /// This is how every parquet read begins: the footer sits at the end and its own
    /// length is in the final eight bytes. Asking for a suffix directly saves a HEAD
    /// request before the GET.
    ///
    /// A `length` larger than the object returns the whole object rather than failing,
    /// because a caller guessing generously at a footer size is doing the right thing.
    ///
    /// # Errors
    ///
    /// As [`Self::read_range`].
    async fn read_suffix(&self, length: u64) -> Result<Bytes, IoError> {
        let size = self.size().await?;
        let start = size.saturating_sub(length);
        self.read_range(start..size).await
    }
}

/// Validate a range against a known object size.
///
/// Shared by every implementation so the boundary rules cannot drift between them.
pub(crate) fn check_range(range: &Range<u64>, size: u64) -> Result<(), IoError> {
    if range.start > range.end || range.end > size {
        return Err(IoError::OutOfBounds {
            start: range.start,
            end: range.end,
            size,
        });
    }
    Ok(())
}
