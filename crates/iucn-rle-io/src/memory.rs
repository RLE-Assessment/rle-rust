//! Bytes already in memory.

use core::ops::Range;

use bytes::Bytes;

use crate::{check_range, ByteSource, IoError};

/// A [`ByteSource`] over bytes the caller already has.
///
/// Two jobs. It makes every format test runnable with no network, no fixtures on
/// disk and no flakiness — which matters because the parsers above are where the
/// subtle bugs live, not the transport. And it is the bring-your-own-data path: a
/// caller who fetched a file some other way can still use the readers.
///
/// ```
/// use futures::executor::block_on;
/// use iucn_rle_io::{ByteSource, InMemorySource};
///
/// let source = InMemorySource::new(b"hello world".to_vec());
/// let bytes = block_on(source.read_range(6..11)).unwrap();
/// assert_eq!(&bytes[..], b"world");
/// ```
#[derive(Clone, Debug)]
pub struct InMemorySource {
    bytes: Bytes,
}

impl InMemorySource {
    /// Wrap a buffer.
    #[must_use]
    pub fn new(bytes: impl Into<Bytes>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl ByteSource for InMemorySource {
    async fn size(&self) -> Result<u64, IoError> {
        Ok(self.bytes.len() as u64)
    }

    async fn read_range(&self, range: Range<u64>) -> Result<Bytes, IoError> {
        let size = self.bytes.len() as u64;
        check_range(&range, size)?;

        // Slicing `Bytes` is refcounted, so this copies nothing.
        let start = usize::try_from(range.start).map_err(|_| IoError::OutOfBounds {
            start: range.start,
            end: range.end,
            size,
        })?;
        let end = usize::try_from(range.end).map_err(|_| IoError::OutOfBounds {
            start: range.start,
            end: range.end,
            size,
        })?;

        Ok(self.bytes.slice(start..end))
    }
}
