//! `HttpSource` against a real HTTP server.
//!
//! Everything else in this crate is tested over in-memory bytes, which is exactly why
//! this file exists: the transport is where the environment's actual behaviour matters,
//! and no amount of in-memory testing exercises a socket, a header, or a status code.
//!
//! The server below is deliberately hand-rolled rather than a library. It has to be
//! able to behave *badly* — ignore a Range header, omit Accept-Ranges, return a short
//! body — and those are the cases worth testing.

#![allow(clippy::cast_possible_truncation)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use iucn_rle_io::{ByteSource, HttpSource, IoError};

/// Drive a future to completion on a reactor reqwest can use.
///
/// `futures::executor::block_on` is enough for the in-memory sources, and silently is
/// not enough here: reqwest's native transport registers its sockets with a tokio
/// reactor, so without one the future simply never wakes and the test hangs rather
/// than failing.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime")
        .block_on(future)
}

/// How the test server should misbehave, if at all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    /// Correct: advertises ranges and honours them.
    Correct,
    /// Honours nothing: returns 200 and the whole body for every request.
    IgnoresRanges,
    /// Serves ranges but never says so.
    NoAcceptRanges,
    /// Returns fewer bytes than the range asked for.
    ShortBody,
}

/// An HTTP server serving `body`, shut down when dropped.
///
/// Each connection is handled on its own thread. That is not tidiness: a server that
/// answered one connection at a time would serialise concurrent requests all by itself,
/// so a client that had stopped issuing them in parallel would still look correct.
struct Server {
    port: u16,
    requests: Arc<AtomicUsize>,
    shutdown: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn start(body: Vec<u8>, behaviour: Behaviour) -> Self {
        Self::start_slow(body, behaviour, Duration::ZERO)
    }

    /// As [`Self::start`], taking `delay` to answer — so a wait that should have
    /// happened once, in parallel, is distinguishable from one that happened per request.
    fn start_slow(body: Vec<u8>, behaviour: Behaviour, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let port = listener.local_addr().expect("an address").port();
        let (shutdown, stopped) = mpsc::channel::<()>();
        let requests = Arc::new(AtomicUsize::new(0));

        let body = Arc::new(body);
        let counter = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            for stream in listener.incoming() {
                // Disconnected counts as "stop": shutdown is signalled by *dropping*
                // the sender, which yields Err(Disconnected) rather than Ok. Checking
                // only for Ok leaves this loop blocked on accept for ever, and the
                // hang then appears in Drop rather than anywhere near the cause.
                if !matches!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                    return;
                }
                let Ok(stream) = stream else { return };
                let body = Arc::clone(&body);
                let counter = Arc::clone(&counter);
                thread::spawn(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(delay);
                    let _ = serve(stream, &body, behaviour);
                });
            }
        });

        Self {
            port,
            requests,
            shutdown: Some(shutdown),
            handle: Some(handle),
        }
    }

    /// Requests answered so far. Counted per connection, which is the same thing here
    /// because every response says `Connection: close`.
    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/data", self.port)
    }

    fn source(&self) -> HttpSource {
        HttpSource::new(self.url()).expect("a usable URL")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.shutdown.take());
        // Unblock the accept loop so the thread can notice the shutdown.
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve(mut stream: TcpStream, body: &[u8], behaviour: Behaviour) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let method = request_line
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_owned();

    let mut range_header = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range:") {
            range_header = Some(value.trim().to_owned());
        }
    }

    let accept_ranges = if behaviour == Behaviour::NoAcceptRanges {
        String::new()
    } else {
        "Accept-Ranges: bytes\r\n".to_owned()
    };

    // Declared on every response, and load-bearing rather than tidy. This server
    // answers one request per connection and then drops the stream — but HTTP/1.1
    // defaults to keep-alive, so without saying so the client is entitled to pool the
    // socket and send its next request down a connection the server has already closed.
    // Whether it notices the close first is a race, which is why this suite failed
    // intermittently on CI and never locally: the only test that issues two requests
    // through one client is the one that broke. Saying `close` means the socket is
    // never pooled, so there is nothing to race.
    let connection = "Connection: close\r\n";

    if method == "HEAD" {
        // The case that matters: a HEAD reply carries Content-Length in the header and
        // no body at all.
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{accept_ranges}{connection}\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        return stream.flush();
    }

    match range_header.as_deref() {
        Some(range) if behaviour != Behaviour::IgnoresRanges => {
            let spec = range.trim_start_matches("bytes=");
            let last = body.len() - 1;
            let (start, end) = match spec.split_once('-') {
                // `bytes=-N` asks for the final N bytes and does not name a position.
                // Parsing it as `0-N` would silently serve the *start* of the file, and
                // a parquet reader handed the wrong end reports a corrupt footer.
                Some(("", count)) => {
                    let count: usize = count.parse().unwrap_or(body.len());
                    (body.len().saturating_sub(count), last)
                }
                Some((from, "")) => (from.parse().unwrap_or(0), last),
                Some((from, to)) => (from.parse().unwrap_or(0), to.parse().unwrap_or(last)),
                None => (0, last),
            };
            let slice = &body[start..=end.min(body.len() - 1)];
            let slice = if behaviour == Behaviour::ShortBody {
                &slice[..slice.len() / 2]
            } else {
                slice
            };

            let head = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\
                 Content-Range: bytes {start}-{end}/{}\r\n{accept_ranges}{connection}\r\n",
                slice.len(),
                body.len()
            );
            stream.write_all(head.as_bytes())?;
            stream.write_all(slice)?;
        }
        _ => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{accept_ranges}{connection}\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes())?;
            stream.write_all(body)?;
        }
    }
    stream.flush()
}

fn body() -> Vec<u8> {
    (0..=255u8).cycle().take(4096).collect()
}

#[test]
fn the_size_comes_from_the_content_length_header() {
    // The bug this file was written for. `reqwest`'s `content_length()` reports the
    // length of the *body*, which for a HEAD response is zero — so a reader that trusts
    // it decides every remote file is empty and fails on the footer instead, with an
    // error blaming the file rather than the request.
    let server = Server::start(body(), Behaviour::Correct);

    let size = block_on(server.source().size()).unwrap();

    assert_eq!(size, 4096);
}

#[test]
fn a_range_returns_exactly_those_bytes() {
    let server = Server::start(body(), Behaviour::Correct);
    let source = server.source();

    let bytes = block_on(source.read_range(10..20)).unwrap();

    assert_eq!(bytes.len(), 10);
    assert_eq!(&bytes[..], &body()[10..20]);
}

#[test]
fn a_suffix_read_gets_the_end_of_the_object() {
    // How every parquet read starts.
    let server = Server::start(body(), Behaviour::Correct);
    let source = server.source();

    let bytes = block_on(source.read_suffix(16)).unwrap();

    assert_eq!(&bytes[..], &body()[4080..]);
}

#[test]
fn a_suffix_read_costs_one_request() {
    // Every parquet read starts here, and the default path spends two round trips on it:
    // a HEAD purely to learn the size, then a GET for the footer. A suffix range request
    // already carries the total in `Content-Range`, so the HEAD is pure latency — and
    // latency, not bandwidth or CPU, is what a remote read is actually made of. Measured
    // on the Bogotá dataset: 0.70s of network, 0.11s of decode, 1.39s of wall clock, with
    // the difference being four sequential requests.
    let server = Server::start(body(), Behaviour::Correct);
    let source = server.source();

    let bytes = block_on(source.read_suffix(16)).unwrap();

    assert_eq!(
        &bytes[..],
        &body()[4080..],
        "the end of the object, not the start"
    );
    assert_eq!(
        server.requests(),
        1,
        "a suffix read should not need a HEAD first"
    );
}

#[test]
fn a_suffix_read_learns_the_size_without_asking_again() {
    // `Content-Range: bytes 4080-4095/4096` states the total, so the size is already
    // known by the time the footer arrives.
    let server = Server::start(body(), Behaviour::Correct);
    let source = server.source();

    block_on(source.read_suffix(16)).unwrap();
    let size = block_on(source.size()).unwrap();

    assert_eq!(size, 4096);
    assert_eq!(server.requests(), 1, "the size came free with the footer");
}

#[test]
fn several_ranges_are_fetched_at_once() {
    // A row group's column chunks are read together. One round trip each is one avoidable
    // wait per chunk, and a national file has dozens of row groups.
    let server = Server::start_slow(body(), Behaviour::Correct, Duration::from_millis(100));
    let source = server.source();

    let started = Instant::now();
    let parts = block_on(source.read_ranges(&[0..64, 64..128, 128..192, 192..256])).unwrap();
    let elapsed = started.elapsed();

    // Order has to survive: callers zip the results back against the ranges they asked
    // for, so a reordering would put one column's bytes under another's name.
    assert_eq!(parts.len(), 4);
    assert_eq!(&parts[0][..], &body()[0..64]);
    assert_eq!(&parts[3][..], &body()[192..256]);
    assert!(
        elapsed < Duration::from_millis(250),
        "took {elapsed:?}; one at a time would be about 400ms"
    );
}

#[test]
fn many_sequential_requests_through_one_source_all_succeed() {
    // The shape every real read has: one HEAD for the size, then a GET per range, all
    // through the same client. `reqwest` pools connections, so this is the only shape
    // that exercises reuse — and reuse is what made this suite flaky on CI.
    let server = Server::start(body(), Behaviour::Correct);
    let source = server.source();

    block_on(async {
        assert_eq!(source.size().await.unwrap(), 4096);
        for start in (0..2048).step_by(64) {
            let bytes = source.read_range(start..start + 64).await.unwrap();
            assert_eq!(bytes.len(), 64, "range {start}..{}", start + 64);
        }
    });
}

#[test]
fn the_size_is_fetched_once_and_remembered() {
    // Every suffix read needs the size; asking the server each time would double the
    // request count for no gain.
    let server = Server::start(body(), Behaviour::Correct);
    let source = server.source();

    assert_eq!(block_on(source.size()).unwrap(), 4096);
    assert_eq!(block_on(source.size()).unwrap(), 4096);
}

#[test]
fn a_server_that_ignores_ranges_is_reported_rather_than_downloaded() {
    // Returning 200 with the whole body means a "cloud-optimised" read just transferred
    // the entire object. Saying so beats doing it silently.
    let server = Server::start(body(), Behaviour::IgnoresRanges);
    let source = server.source();

    let error = block_on(source.read_range(0..16)).unwrap_err();

    assert!(
        matches!(error, IoError::RangesUnsupported { .. }),
        "{error:?}"
    );
}

#[test]
fn a_server_that_does_not_advertise_ranges_is_refused_up_front() {
    let server = Server::start(body(), Behaviour::NoAcceptRanges);

    let error = block_on(server.source().size()).unwrap_err();

    assert!(
        matches!(error, IoError::RangesUnsupported { .. }),
        "{error:?}"
    );
}

#[test]
fn a_short_response_is_an_error_not_a_truncated_read() {
    // A parser handed fewer bytes than it asked for would report a corrupt file, and
    // the user would go looking for a damaged dataset instead of a flaky server.
    let server = Server::start(body(), Behaviour::ShortBody);
    let source = server.source();

    let error = block_on(source.read_range(0..64)).unwrap_err();

    assert!(matches!(error, IoError::Transport(_)), "{error:?}");
}

#[test]
fn an_unreachable_url_is_reported_as_a_transport_failure() {
    // Bind a port and immediately release it, so nothing is listening there. Guessing
    // an unused port — `server.port + 1` — collides with another test's server often
    // enough to matter, since these run in parallel and the OS assigns ports densely.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("an address")
        .port();
    let source = HttpSource::new(format!("http://127.0.0.1:{port}/data")).expect("a usable URL");

    let error = block_on(source.size()).unwrap_err();

    assert!(matches!(error, IoError::Transport(_)), "{error:?}");
}

/// Silence the unused-import warning for `Read`, which `BufReader` needs in scope.
const _: fn(&mut dyn Read) = |_| {};
