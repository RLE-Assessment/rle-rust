"""The Python runner for rle-python's golden null-island dataset.

Runs the identical fixture as the Rust, R and JavaScript suites, so agreement with
the implementation assessments currently use is enforced on every surface rather
than checked once by hand.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

import iucn_rle

FIXTURE = (
    Path(__file__).resolve().parents[3]
    / "fixtures"
    / "cases"
    / "null_island_aoo.json"
)


def load() -> dict:
    with FIXTURE.open() as f:
        return json.load(f)


def metrics() -> dict:
    fixture = load()
    polygons = [(f["code"], f["rings"]) for f in fixture["features"]]
    result = iucn_rle.distribution_metrics(polygons)
    return {e["ecosystem"]: e for e in result["ecosystems"]}


@pytest.mark.unit
def test_fixture_is_present() -> None:
    # A missing fixture must fail loudly rather than vacuously pass.
    assert FIXTURE.exists(), f"golden fixture not found at {FIXTURE}"
    assert len(load()["features"]) == 3


@pytest.mark.unit
def test_occupied_cell_counts_match_rle_python() -> None:
    fixture = load()
    by_ecosystem = metrics()

    for feature in fixture["features"]:
        expected = sum(
            1
            for cell in fixture["expected_cells"]
            if cell["fractions"].get(feature["code"], 0.0) > 0.0
        )
        assert by_ecosystem[feature["code"]]["occupied_cell_count"] == expected


@pytest.mark.unit
def test_published_metrics_match() -> None:
    # From the committed workshop notebook: "EOO is 73.2 km2", "AOO is 4 cells".
    published = load()["published_metrics"]
    by_ecosystem = metrics()

    for code, expected in published["ecosystems"].items():
        actual = by_ecosystem[code]
        assert abs(actual["eoo_km2"] - expected["eoo_km2"]) < published[
            "eoo_tolerance_km2"
        ], f"{code} EOO"
        assert actual["aoo_cells"] == expected["aoo_cells"], f"{code} AOO"


@pytest.mark.unit
def test_out_of_range_coordinates_are_rejected() -> None:
    # Rejected rather than clamped: swapped or already-projected coordinates would
    # otherwise yield a plausible-looking but wrong AOO.
    with pytest.raises(ValueError, match="latitude"):
        iucn_rle.distribution_metrics([("forest", [[[10.0, 200.0], [11.0, 201.0]]])])
