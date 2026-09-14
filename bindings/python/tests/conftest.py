"""Fixtures serving GeoParquet over HTTP, for the remote-reading tests.

The server itself lives in ``range_server.py`` and runs as a child process — see that
module for why a thread will not do.
"""

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

import pytest

FIXTURES = Path(__file__).resolve().parents[3] / "fixtures" / "data"
SERVER = Path(__file__).resolve().parent / "range_server.py"


@dataclass
class Served:
    """A fixture file reachable over HTTP for the lifetime of a test."""

    url: str
    process: subprocess.Popen


def start(name: str, delay: float) -> Served:
    path = FIXTURES / name
    assert path.exists(), (
        f"fixture not found at {path}; "
        "regenerate with `python3 tools/generate_geoparquet_fixture.py`"
    )

    process = subprocess.Popen(
        [sys.executable, str(SERVER), str(path), str(delay)],
        stdout=subprocess.PIPE,
        text=True,
    )
    # The child picks its own port and announces it, so parallel test runs cannot
    # collide on a number chosen in advance.
    line = process.stdout.readline().strip()
    assert line.isdigit(), f"server did not report a port, said: {line!r}"

    return Served(url=f"http://127.0.0.1:{line}/data.parquet", process=process)


def stop(served: Served) -> None:
    served.process.terminate()
    try:
        served.process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        served.process.kill()
    if served.process.stdout is not None:
        served.process.stdout.close()


@pytest.fixture
def served():
    """The four-ecosystem fixture, answering as fast as the loopback allows."""
    server = start("ecosystems.parquet", 0.0)
    yield server
    stop(server)


@pytest.fixture
def slow_served():
    """The same file, answering slowly enough that a blocked main thread is visible."""
    server = start("ecosystems.parquet", 0.05)
    yield server
    stop(server)
