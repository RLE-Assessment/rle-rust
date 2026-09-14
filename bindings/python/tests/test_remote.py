"""Reading a remote GeoParquet dataset from Python.

Two claims are being tested, and only one of them is about numbers.

The first is that the metrics are right — same ecosystems, same features, and the
same answers whether or not the reader pruned row groups it proved irrelevant.

The second is that the call **releases the GIL**. That is a contract rather than an
optimisation: Quarto and Jupyter kernels already run an asyncio event loop, and a
binding that held the GIL through a two-minute national read would freeze the whole
kernel. It is also invisible in ordinary use — the numbers come out identical either
way — so it needs a test that fails loudly when it regresses.
"""

from __future__ import annotations

import threading
import time

import pytest

import iucn_rle

ECO_COLUMN = "eco_code"


@pytest.mark.unit
def test_reads_every_feature_over_http(served) -> None:
    result = iucn_rle.distribution_metrics_from_url(served.url, ECO_COLUMN)

    assert [e["ecosystem"] for e in result["ecosystems"]] == [
        "ECO_A",
        "ECO_B",
        "ECO_C",
        "ECO_D",
    ]
    assert result["read"]["features"] == 16
    assert result["read"]["row_groups_read"] == 4
    assert result["read"]["row_groups_skipped"] == 0
    assert result["grid_crs"] == "ESRI:54034"


@pytest.mark.unit
def test_the_metrics_are_real_numbers(served) -> None:
    # A read that fetched nothing would report success with an empty accumulator, so
    # the values have to be checked and not just the shape.
    result = iucn_rle.distribution_metrics_from_url(served.url, ECO_COLUMN)
    by_code = {e["ecosystem"]: e for e in result["ecosystems"]}

    assert by_code["ECO_C"]["eoo_km2"] > 0.0
    assert by_code["ECO_C"]["aoo_cells"] > 0


@pytest.mark.unit
def test_pruning_does_not_change_the_answer(served) -> None:
    # The point of the read report: a filtered read must touch less of the file while
    # producing metrics identical to the unfiltered one.
    whole = iucn_rle.distribution_metrics_from_url(served.url, ECO_COLUMN)
    filtered = iucn_rle.distribution_metrics_from_url(
        served.url, ECO_COLUMN, ecosystems=["ECO_B"]
    )

    expected = next(e for e in whole["ecosystems"] if e["ecosystem"] == "ECO_B")
    assert filtered["ecosystems"] == [expected]
    assert filtered["read"]["row_groups_read"] == 1
    assert filtered["read"]["row_groups_skipped"] == 3
    assert filtered["read"]["bytes_fetched"] < whole["read"]["bytes_fetched"]


@pytest.mark.unit
def test_a_bounding_box_restricts_the_read(served) -> None:
    # Far out in the Pacific, where the fixture has nothing.
    empty = iucn_rle.distribution_metrics_from_url(
        served.url, ECO_COLUMN, bbox=(-170.0, -60.0, -160.0, -50.0)
    )

    assert empty["ecosystems"] == []
    assert empty["read"]["features"] == 0


@pytest.mark.unit
def test_the_declared_crs_is_reported(served) -> None:
    # Provenance, not decoration: the metrics project from longitude/latitude assuming
    # WGS84, and real national data arrives on its own datum.
    result = iucn_rle.distribution_metrics_from_url(served.url, ECO_COLUMN)

    assert result["read"]["crs"] is not None


@pytest.mark.unit
def test_the_gil_is_released_during_a_remote_read(slow_served) -> None:
    """The contract. Without ``py.detach`` the main thread makes no progress at all.

    The worker does a genuinely slow read while the main thread counts how often it
    gets to run. Holding the GIL across the read starves it completely, so the tick
    count separates the two cases by orders of magnitude rather than by a margin.
    """
    done = threading.Event()
    result: dict = {}

    def read() -> None:
        try:
            result["value"] = iucn_rle.distribution_metrics_from_url(
                slow_served.url, ECO_COLUMN
            )
        finally:
            done.set()

    worker = threading.Thread(target=read)
    started = time.perf_counter()
    worker.start()

    ticks = 0
    while not done.wait(0.001):
        ticks += 1
    worker.join(timeout=30)
    elapsed = time.perf_counter() - started

    assert elapsed > 0.2, (
        f"the read finished in {elapsed:.3f}s, too fast for this test to mean "
        "anything; the fixture server is meant to answer slowly"
    )
    assert ticks > 20, (
        f"the main thread ran only {ticks} times during a {elapsed:.3f}s read, "
        "which means the GIL was held across it"
    )
    assert result["value"]["read"]["features"] == 16, "and the work really happened"


@pytest.mark.unit
def test_an_unknown_column_lists_the_columns_that_exist(served) -> None:
    # The likeliest user error: every national dataset spells this column differently.
    with pytest.raises(ValueError, match="eco_code"):
        iucn_rle.distribution_metrics_from_url(served.url, "ECOSYSTEM")


@pytest.mark.unit
def test_a_server_that_is_not_there_raises_an_io_error(served) -> None:
    # Port 1 is privileged and unbound, so this is refused rather than hanging.
    with pytest.raises(OSError, match="127.0.0.1"):
        iucn_rle.distribution_metrics_from_url(
            "http://127.0.0.1:1/data.parquet", ECO_COLUMN
        )
