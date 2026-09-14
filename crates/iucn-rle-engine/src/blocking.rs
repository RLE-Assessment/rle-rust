//! Synchronous entry points, for callers that cannot be asynchronous.
//!
//! Python, R and the CLI all want to call a function and get an answer. The engine is
//! asynchronous because fetching bytes is, so somewhere a runtime has to be driven and
//! blocked on. Doing that once here rather than in each binding is what keeps the
//! bindings thin, and it means the GIL contract below has a single place to be right.
//!
//! Native only. WebAssembly has no blocking primitive — a browser tab cannot park its
//! only thread — so the WASM binding stays on the async API and returns a promise.
//!
//! # Why a fresh runtime per call
//!
//! Every future in this project is `?Send`, so the runtime must be current-thread.
//! Sharing one across calls would then serialise them: two threads blocking on the same
//! current-thread runtime take turns rather than overlapping, which would silently undo
//! the concurrency that releasing the GIL exists to buy. Building a runtime costs tens
//! of microseconds against a read measured in seconds, so per-call is both correct and
//! cheap.

use iucn_rle_core::distribution::DistributionAccumulator;
use iucn_rle_core::ffi::{summarize, DistributionSummary};
use iucn_rle_format::geoparquet::Query;
use iucn_rle_io::{ByteSource, HttpSource};
use serde::Serialize;

use crate::{accumulate_geoparquet_with, url_hint, EngineError, ReadOptions, ReadReport};

/// Metrics from a remote dataset, with a record of what reading them cost.
///
/// The metrics are flattened so this is the same JSON shape a caller gets from local
/// polygons, plus a `read` key. Anything downstream that consumes one consumes the other.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteMetrics {
    /// The Criterion B spatial metrics.
    #[serde(flatten)]
    pub metrics: DistributionSummary,
    /// What the read actually did — row groups touched, bytes fetched, CRS declared.
    pub read: ReadReport,
}

/// Compute Criterion B spatial metrics from a `GeoParquet` file at a URL.
///
/// Blocks until the read completes. Callers holding a lock — the GIL, R's evaluator —
/// must release it around this call: it is dominated by network waiting, and holding an
/// interpreter still for the duration of a national read freezes it for minutes.
///
/// # Errors
///
/// See [`EngineError`]. Structural problems are reported from the footer, before any
/// data is fetched, so a mistyped column costs one small request rather than a download.
pub fn distribution_metrics_from_url(
    url: &str,
    ecosystem_column: &str,
    query: &Query,
    options: &ReadOptions,
) -> Result<RemoteMetrics, EngineError> {
    let source = HttpSource::new(url)?;
    distribution_metrics_from_source(&source, ecosystem_column, query, options)
}

/// As [`distribution_metrics_from_url`], from any byte source.
///
/// Exists so the blocking path is testable without a network: the tests drive it over
/// an in-memory source, and `from_url` adds nothing but an [`HttpSource`].
///
/// # Errors
///
/// See [`EngineError`].
pub fn distribution_metrics_from_source<S: ByteSource + ?Sized>(
    source: &S,
    ecosystem_column: &str,
    query: &Query,
    options: &ReadOptions,
) -> Result<RemoteMetrics, EngineError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            EngineError::Io(iucn_rle_io::IoError::Transport(format!(
                "could not start an async runtime: {error}"
            )))
        })?;

    let mut accumulator = DistributionAccumulator::new();
    let report = runtime.block_on(accumulate_geoparquet_with(
        source,
        query,
        ecosystem_column,
        &mut accumulator,
        options,
    ))?;

    Ok(RemoteMetrics {
        metrics: summarize(&accumulator.finish()),
        read: report,
    })
}

/// A failure message for a URL, with advice when the URL's shape suggests a cause.
///
/// Bindings can only carry a string across their boundary, so the hint has to be part of
/// the message rather than a field a caller might never look at. Pasting a data portal's
/// web address instead of its file address is the most common way this fails, and the
/// underlying error — HTML where parquet was expected — describes the symptom only.
#[must_use]
pub fn describe_failure(url: &str, error: &EngineError) -> String {
    url_hint(url).map_or_else(
        || error.to_string(),
        |hint| format!("{error}\n\nhint: {hint}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_failure_is_reported_as_it_is() {
        let error = EngineError::NotGeographic {
            found: "EPSG:3857".to_owned(),
        };

        let message = describe_failure("https://example.org/data.parquet", &error);

        assert!(message.contains("EPSG:3857"));
        assert!(!message.contains("hint:"), "{message}");
    }

    #[test]
    fn a_portal_web_url_is_answered_with_the_data_host() {
        // The mistake a user actually made: source.coop's browsable interface and its
        // bytes live at the same path on different hosts.
        let error = EngineError::Format(iucn_rle_format::geoparquet::FormatError::NotParquet {
            reason: "the file does not end with the parquet magic PAR1".to_owned(),
        });

        let message = describe_failure("https://source.coop/tyler/x/y.parquet", &error);

        assert!(message.contains("hint:"), "{message}");
        assert!(message.contains("data.source.coop"), "{message}");
    }
}
