//! Bytes over HTTP, on native and in the browser.

use core::ops::Range;
use std::cell::RefCell;

use bytes::Bytes;
use futures_util::stream::{self, StreamExt, TryStreamExt};

use crate::{ByteSource, IoError};

/// How many range requests to have outstanding at once.
///
/// Enough to hide latency, few enough not to look like an attack. Browsers cap
/// concurrent connections per host at around six, and the returns fall off well before
/// that anyway: fetching a 16 MB object as four parallel ranges took 0.61 s against
/// 0.70 s sequentially, so this buys round trips rather than bandwidth.
const MAX_CONCURRENT_RANGES: usize = 8;

/// A [`ByteSource`] backed by HTTP range requests.
///
/// `reqwest` is the one client covering both targets: on `wasm32-unknown-unknown` it
/// dispatches through the browser Fetch API, so the same code serves a CLI and a web
/// page.
///
/// # Browser requirements
///
/// A server must send `Accept-Ranges` and honour `Range`. For browser use it must
/// **also** expose `Content-Range` through CORS — `Content-Length` alone is
/// CORS-safelisted and `Content-Range` is not, so without it the browser cannot read
/// the size of a partial response and range reads fail in a way that looks like a
/// corrupt file.
pub struct HttpSource {
    client: reqwest::Client,
    url: String,
    /// Cached object size. `RefCell` rather than a lock because every future here is
    /// `?Send` and confined to one thread by construction.
    size: RefCell<Option<u64>>,
}

impl HttpSource {
    /// A source reading from `url`.
    ///
    /// # Errors
    ///
    /// [`IoError::Transport`] if an HTTP client cannot be constructed.
    pub fn new(url: impl Into<String>) -> Result<Self, IoError> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| IoError::Transport(format!("could not build HTTP client: {e}")))?;
        Ok(Self {
            client,
            url: url.into(),
            size: RefCell::new(None),
        })
    }

    /// The URL being read.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

#[async_trait::async_trait(?Send)]
impl ByteSource for HttpSource {
    async fn size(&self) -> Result<u64, IoError> {
        if let Some(size) = *self.size.borrow() {
            return Ok(size);
        }

        let response = self
            .client
            .head(&self.url)
            .send()
            .await
            .map_err(|e| IoError::Transport(format!("HEAD {}: {e}", self.url)))?;

        if !response.status().is_success() {
            return Err(IoError::Transport(format!(
                "HEAD {} returned {}",
                self.url,
                response.status()
            )));
        }

        // A server that does not advertise range support would silently return the
        // whole object for every request, turning a cloud-optimised read into a full
        // download. Say so rather than quietly transferring gigabytes.
        let accepts_ranges = response
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
        if !accepts_ranges {
            return Err(IoError::RangesUnsupported {
                url: self.url.clone(),
            });
        }

        // Read the header, not `content_length()`. The latter reports the length of the
        // *body*, and a HEAD response has none — so it answers 0 for every object, and
        // the failure surfaces later as "this file is too short to be parquet", blaming
        // the file for a mistake made in the request.
        let size = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| {
                IoError::Transport(format!(
                    "HEAD {} gave no usable Content-Length, so the object's size is \
                     unknown and its footer cannot be located",
                    self.url
                ))
            })?;

        *self.size.borrow_mut() = Some(size);
        Ok(size)
    }

    async fn read_range(&self, range: Range<u64>) -> Result<Bytes, IoError> {
        if range.start > range.end {
            return Err(IoError::OutOfBounds {
                start: range.start,
                end: range.end,
                size: self.size.borrow().unwrap_or(0),
            });
        }
        if range.start == range.end {
            return Ok(Bytes::new());
        }

        // HTTP ranges are inclusive at both ends; Rust's are half-open.
        let header = format!("bytes={}-{}", range.start, range.end - 1);

        let response = self
            .client
            .get(&self.url)
            .header(reqwest::header::RANGE, &header)
            .send()
            .await
            .map_err(|e| IoError::Transport(format!("GET {} {header}: {e}", self.url)))?;

        let status = response.status();

        // 200 means the server ignored the Range header and is sending everything.
        // Accepting that would silently download the whole object.
        if status == reqwest::StatusCode::OK {
            return Err(IoError::RangesUnsupported {
                url: self.url.clone(),
            });
        }
        if status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(IoError::Transport(format!(
                "GET {} {header} returned {status}",
                self.url
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| IoError::Transport(format!("reading {} {header}: {e}", self.url)))?;

        let wanted = range.end - range.start;
        if bytes.len() as u64 != wanted {
            return Err(IoError::Transport(format!(
                "GET {} {header} returned {} bytes, expected {wanted}",
                self.url,
                bytes.len()
            )));
        }

        Ok(bytes)
    }

    async fn read_ranges(&self, ranges: &[Range<u64>]) -> Result<Vec<Bytes>, IoError> {
        // Issued together rather than one after another. The default implementation
        // waits for each response before sending the next request, so a row group's
        // column chunks cost one round trip each — and a remote read is made of round
        // trips far more than of bytes. Measured on the Bogotá dataset: 0.70 s to move
        // the data, 0.11 s to decode it, and 1.39 s of wall clock, the difference being
        // four sequential requests.
        //
        // `buffered` preserves order, which is load-bearing: callers zip the results
        // back against the ranges they asked for, so a reordering would file one
        // column's bytes under another's name.
        stream::iter(ranges.iter().cloned().map(|range| self.read_range(range)))
            .buffered(MAX_CONCURRENT_RANGES)
            .try_collect()
            .await
    }

    async fn read_suffix(&self, length: u64) -> Result<Bytes, IoError> {
        if length == 0 {
            return Ok(Bytes::new());
        }

        // A suffix range asks for the last N bytes without knowing the size, and the
        // response states the total in `Content-Range`. The default path instead spends
        // a whole round trip on a HEAD whose only purpose is to work out where to ask
        // from — before any data moves at all.
        let header = format!("bytes=-{length}");
        let response = self
            .client
            .get(&self.url)
            .header(reqwest::header::RANGE, &header)
            .send()
            .await
            .map_err(|e| IoError::Transport(format!("GET {} {header}: {e}", self.url)))?;

        let status = response.status();
        if status == reqwest::StatusCode::OK {
            return Err(IoError::RangesUnsupported {
                url: self.url.clone(),
            });
        }
        if status != reqwest::StatusCode::PARTIAL_CONTENT {
            // Not every server implements suffix ranges. Falling back costs the round
            // trip this exists to save, which is better than failing on a file that a
            // two-request reader could have read.
            let size = self.size().await?;
            let start = size.saturating_sub(length);
            return self.read_range(start..size).await;
        }

        if let Some(total) = total_from_content_range(&response) {
            *self.size.borrow_mut() = Some(total);
        }

        response
            .bytes()
            .await
            .map_err(|e| IoError::Transport(format!("reading {} {header}: {e}", self.url)))
    }
}

/// The object's total size, from a `Content-Range: bytes 4080-4095/4096` header.
///
/// `None` when the header is absent, unparsable, or says `*` — a server is allowed to
/// omit the total, and guessing one would be worse than asking.
fn total_from_content_range(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?
        .rsplit_once('/')?
        .1
        .trim()
        .parse()
        .ok()
}
