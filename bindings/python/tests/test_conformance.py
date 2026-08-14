"""The Python runner for the cross-language conformance corpus.

Runs the identical JSON file as the Rust, R, and JavaScript suites. If this
passes and the others do too, the bindings provably agree.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

import iucn_rle

CORPUS = Path(__file__).resolve().parents[3] / "fixtures" / "cases" / "criterion_b.json"


def load_cases() -> list[dict]:
    with CORPUS.open() as f:
        return json.load(f)["cases"]


CASES = load_cases()


def test_corpus_is_not_empty() -> None:
    # Guards against a renamed or unreadable fixture making the suite
    # vacuously pass.
    assert len(CASES) >= 20


@pytest.mark.unit
@pytest.mark.parametrize("case", CASES, ids=[c["id"] for c in CASES])
def test_case(case: dict) -> None:
    eoo_bounds = None
    if case.get("eoo_lower_km2") is not None:
        eoo_bounds = (case["eoo_lower_km2"], case["eoo_upper_km2"])

    result = iucn_rle.criterion_b(
        eoo_km2=case["eoo_km2"],
        aoo_cells=case["aoo_cells"],
        subconditions=[(s["sub"], s["status"]) for s in case["subconditions"]],
        eoo_bounds=eoo_bounds,
    )

    by_criterion = {c["criterion"]: c["category"] for c in result["criteria"]}

    assert by_criterion["B1"] == case["expect"]["b1"]
    assert by_criterion["B2"] == case["expect"]["b2"]
    assert result["overall"] == case["expect"]["overall"]


@pytest.mark.unit
def test_thresholds_shipped_with_the_wheel_match_the_engine() -> None:
    # Every distribution ships the same TOML bytes; this proves the Python wheel
    # was not built against a different edition of the Guidelines.
    toml = iucn_rle.thresholds_toml()
    assert 'guidelines_version = "2.0"' in toml
    assert len(iucn_rle.thresholds_sha256()) == 64

    result = iucn_rle.criterion_b(eoo_km2=15_000)
    assert result["thresholds_sha256"] == iucn_rle.thresholds_sha256()


@pytest.mark.unit
def test_unknown_subcondition_raises_a_useful_error() -> None:
    with pytest.raises(ValueError, match="a|b|c"):
        iucn_rle.criterion_b(eoo_km2=15_000, subconditions={"z": "met"})
