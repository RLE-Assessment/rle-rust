//! Bytes over HTTP, on native and in the browser.

use core::ops::Range;
use std::cell::RefCell;

use bytes::Bytes;

use crate::{ByteSource, IoError};

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

        let size = response
            .content_length()
            .ok_or_else(|| IoError::Transport(format!("HEAD {} gave no length", self.url)))?;

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
}
