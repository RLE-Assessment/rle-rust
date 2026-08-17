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
use std::sync::mpsc;
use std::thread;

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

/// A single-threaded HTTP server serving `body`, shut down when dropped.
struct Server {
    port: u16,
    shutdown: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn start(body: Vec<u8>, behaviour: Behaviour) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let port = listener.local_addr().expect("an address").port();
        let (shutdown, stopped) = mpsc::channel::<()>();

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
                if serve(stream, &body, behaviour).is_err() {
                    return;
                }
            }
        });

        Self {
            port,
            shutdown: Some(shutdown),
            handle: Some(handle),
        }
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

    if method == "HEAD" {
        // The case that matters: a HEAD reply carries Content-Length in the header and
        // no body at all.
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{accept_ranges}\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        return stream.flush();
    }

    match range_header.as_deref() {
        Some(range) if behaviour != Behaviour::IgnoresRanges => {
            let spec = range.trim_start_matches("bytes=");
            let (start, end) = spec.split_once('-').unwrap_or(("0", ""));
            let start: usize = start.parse().unwrap_or(0);
            let end: usize = end.parse().unwrap_or(body.len() - 1);
            let slice = &body[start..=end.min(body.len() - 1)];
            let slice = if behaviour == Behaviour::ShortBody {
                &slice[..slice.len() / 2]
            } else {
                slice
            };

            let head = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\
                 Content-Range: bytes {start}-{end}/{}\r\n{accept_ranges}\r\n",
                slice.len(),
                body.len()
            );
            stream.write_all(head.as_bytes())?;
            stream.write_all(slice)?;
        }
        _ => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{accept_ranges}\r\n",
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
