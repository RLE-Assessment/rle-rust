"""The Python runner for the cross-language conformance corpus.

Runs the identical JSON file as the Rust, R, C ABI and JavaScript suites. If this
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
    assert len(CASES) >= 28


@pytest.mark.unit
@pytest.mark.parametrize("case", CASES, ids=[c["id"] for c in CASES])
def test_case(case: dict) -> None:
    eoo_bounds = None
    if case.get("eoo_lower_km2") is not None:
        eoo_bounds = (case["eoo_lower_km2"], case["eoo_upper_km2"])

    result = iucn_rle.criterion_b(
        eoo_km2=case.get("eoo_km2"),
        aoo_cells=case.get("aoo_cells"),
        clauses=[(c["sub"], c["status"]) for c in case.get("clauses", [])],
        locations=case.get("locations"),
        no_plausible_threats=case.get("no_plausible_threats", False),
        locations_insufficient_information=case.get(
            "locations_insufficient_information", False
        ),
        eoo_bounds=eoo_bounds,
    )

    by_criterion = {c["criterion"]: c["category"] for c in result["criteria"]}

    for key, expected in case["expect"].items():
        actual = result["overall"] if key == "overall" else by_criterion[key.upper()]
        assert actual == expected, key

    thresholds = {
        c["criterion"]: c["threshold_category"] for c in result["criteria"]
    }
    for key, expected in case.get("expect_threshold", {}).items():
        assert thresholds[key.upper()] == expected, f"{key} threshold"


@pytest.mark.unit
def test_thresholds_shipped_with_the_wheel_match_the_engine() -> None:
    # Every distribution ships the same TOML bytes; this proves the Python wheel
    # was not built against a different edition of the Guidelines.
    toml = iucn_rle.thresholds_toml()
    assert 'guidelines_version = "2.0"' in toml
    assert 'criteria_version = "2.1"' in toml
    assert len(iucn_rle.thresholds_sha256()) == 64

    result = iucn_rle.criterion_b(eoo_km2=15_000)
    assert result["thresholds_sha256"] == iucn_rle.thresholds_sha256()


@pytest.mark.unit
def test_clause_c_as_a_status_is_rejected() -> None:
    # (c) is a count, not a status. Accepting a boolean here is the bug M1.5 fixed.
    with pytest.raises(ValueError, match="locations"):
        iucn_rle.criterion_b(eoo_km2=15_000, clauses={"c": "met"})


@pytest.mark.unit
def test_unknown_clause_raises_a_useful_error() -> None:
    with pytest.raises(ValueError, match=r"a\|b"):
        iucn_rle.criterion_b(eoo_km2=15_000, clauses={"z": "met"})
