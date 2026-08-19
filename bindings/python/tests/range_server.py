"""A minimal HTTP server that honours range requests, run as a separate process.

Two things about it are deliberate.

**It implements ranges by hand.** Python's own ``http.server`` ignores ``Range`` and
sends the whole file with no ``Accept-Ranges``. A reader tested against that would look
correct while transferring gigabytes, which is the one failure worth catching here.

**It runs in its own process, not a thread.** This is what lets the GIL test work at
all. A server handling requests in the test's own interpreter cannot answer while the
Rust call holds the GIL, so a binding that failed to release it would *deadlock* — a
hang rather than a failed assertion, and one that would stall CI with no useful message.
Across a process boundary the server is unaffected, the read completes normally, and the
missing release shows up as exactly what it is: a main thread that never ran.

Usage, printing the chosen port on stdout so the caller need not guess one::

    python range_server.py <file> <delay-seconds>
"""

from __future__ import annotations

import re
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

RANGE = re.compile(r"^bytes=(\d*)-(\d*)$")


def build_handler(body: bytes, delay: float) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_args) -> None:
            pass  # keep the test output readable

        def _wait(self) -> None:
            if delay:
                time.sleep(delay)

        def do_HEAD(self) -> None:  # noqa: N802 - the stdlib's naming
            self._wait()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Accept-Ranges", "bytes")
            self.end_headers()

        def do_GET(self) -> None:  # noqa: N802 - the stdlib's naming
            self._wait()
            header = self.headers.get("Range")
            match = RANGE.match(header) if header else None

            if match is None:
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Accept-Ranges", "bytes")
                self.end_headers()
                self.wfile.write(body)
                return

            # Both ends are inclusive, per RFC 9110. Treating the end as exclusive
            # silently truncates every response by one byte, which a parquet footer
            # survives just often enough to be confusing.
            #
            # `bytes=-N` is a *suffix*: the final N bytes, naming no position. Reading
            # it as `0-N` serves the start of the file instead, and a reader handed the
            # wrong end reports a corrupt footer rather than a bad request. The engine
            # opens every file this way, to learn the size and the footer in one
            # request instead of two.
            lo, hi = match.group(1), match.group(2)
            if lo == "":
                start, end = max(0, len(body) - int(hi)), len(body) - 1
            else:
                start = int(lo)
                end = min(int(hi) if hi else len(body) - 1, len(body) - 1)
            chunk = body[start : end + 1]

            self.send_response(206)
            self.send_header("Content-Length", str(len(chunk)))
            self.send_header("Content-Range", f"bytes {start}-{end}/{len(body)}")
            self.send_header("Accept-Ranges", "bytes")
            self.end_headers()
            self.wfile.write(chunk)

    return Handler


def main() -> None:
    path, delay = sys.argv[1], float(sys.argv[2])
    with open(path, "rb") as f:
        body = f.read()

    server = ThreadingHTTPServer(("127.0.0.1", 0), build_handler(body, delay))
    print(server.server_address[1], flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
