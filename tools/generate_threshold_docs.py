#!/usr/bin/env python3
"""Generate docs/thresholds.md from the threshold table the library ships.

The point is that the published thresholds cannot drift from the applied ones. The
TOML read here is the identical file `iucn-rle-core` embeds with `include_str!`, and a
Rust test (`tests/thresholds_match_toml.rs`) asserts the compiled constants match it.
So: documentation == data file == compiled engine, with a test on each link.

Hand-maintaining this page would break that chain the first time a number changed.

Usage:
    python3 tools/generate_threshold_docs.py            # write docs/thresholds.md
    python3 tools/generate_threshold_docs.py --check    # fail if out of date (CI)
"""

from __future__ import annotations

import argparse
import hashlib
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOML = ROOT / "crates" / "iucn-rle-core" / "thresholds" / "iucn-rle-v2.0-2024.toml"
OUTPUT = ROOT / "docs" / "thresholds.md"

CATEGORY_NAMES = {
    "CO": "Collapsed",
    "CR": "Critically Endangered",
    "EN": "Endangered",
    "VU": "Vulnerable",
    "NT": "Near Threatened",
    "LC": "Least Concern",
    "DD": "Data Deficient",
    "NE": "Not Evaluated",
}

# Ordered most to least threatened, so generated tables read like the Guidelines'.
CATEGORY_ORDER = list(CATEGORY_NAMES)


def order_key(category: str) -> int:
    return CATEGORY_ORDER.index(category)


def format_number(value: float | int) -> str:
    """Render a bound the way the Guidelines print it."""
    if isinstance(value, float) and value.is_integer():
        value = int(value)
    return f"{value:,}"


def render(table: dict, digest: str) -> str:
    lines: list[str] = []
    add = lines.append

    add("---")
    add("title: Thresholds")
    add("subtitle: Generated from the data file the library ships")
    add("---")
    add("")
    add(
        ":::{important} This page is generated\n"
        "Every number below is read directly from\n"
        "`crates/iucn-rle-core/thresholds/iucn-rle-v2.0-2024.toml`, the same file the\n"
        "engine compiles in. A test asserts the compiled constants match that file, and\n"
        "CI fails if this page falls out of step with it. What you read here is what the\n"
        "library applied.\n"
        ":::"
    )
    add("")
    add(f"**Source.** {table['citation']}")
    add("")
    add(f"- Guidelines version **{table['guidelines_version']}** ({table['guidelines_year']})")
    add(f"- Criteria version **{table['criteria_version']}**")
    add(f"- Threshold table SHA-256: `{digest}`")
    add("")
    add(
        "The digest is recorded in the provenance of every result, so a category can be\n"
        "traced back to the exact numbers that produced it."
    )
    add("")

    for criterion in table["criterion"]:
        cid = criterion["id"]
        add(f"## {cid}")
        add("")
        add(f"*{criterion['metric_description']}*")
        add("")
        add(f"Source: {criterion['source']}.")
        add("")

        breakpoints = criterion.get("breakpoints", [])
        if breakpoints:
            add("| Category | Metric threshold |")
            add("|---|---|")
            for bp in breakpoints:
                name = CATEGORY_NAMES[bp["category"]]
                add(f"| **{bp['category']}** {name} | ≤ {format_number(bp['max'])} |")
            above = criterion["above_all"]
            add(f"| **{above}** {CATEGORY_NAMES[above]} | above every threshold |")
            add("")
            add("Bounds are inclusive: a value equal to a bound meets that category.")
            add("")
        else:
            add(
                "This sub-criterion has no spatial metric; the outcome rests entirely on\n"
                "the sub-conditions below."
            )
            add("")

        bounds = criterion.get("location_bounds", [])
        if bounds:
            heading = (
                "### Clause (c): threat-defined locations"
                if cid != "B3"
                else "### Threat-defined locations"
            )
            add(heading)
            add("")
            add("| Category | Clause is met at |")
            add("|---|---|")
            for bound in sorted(bounds, key=lambda b: order_key(b["category"])):
                count = bound["max_locations"]
                name = CATEGORY_NAMES[bound["category"]]
                phrase = "exactly 1 location" if count == 1 else f"≤ {count} locations"
                add(f"| **{bound['category']}** {name} | {phrase} |")
            add("")
            if cid != "B3":
                add(
                    "This bound is **category dependent**, which is why the library\n"
                    "evaluates each category level separately rather than deriving a\n"
                    "category from the metric and then applying a yes/no gate."
                )
                add("")

        required = criterion.get("requires_any_subcondition", [])
        if required:
            clauses = ", ".join(f"({c})" for c in required)
            add(
                f"A listing under {cid} requires the metric threshold **and** at least one\n"
                f"of clauses {clauses}."
            )
            add("")

        never = criterion.get("never_emit", [])
        if never:
            names = ", ".join(f"**{c}** {CATEGORY_NAMES[c]}" for c in never)
            add(f"{cid} never yields: {names}.")
            add("")

    add("## Not yet transcribed")
    add("")
    add(
        "Criteria A, C, D and E have no table here. Their thresholds are published in\n"
        "Appendix 1 of the Guidelines, but transcribing numbers that no code exercises\n"
        "and no test checks is how an unverified value gets in. They arrive with their\n"
        "own implementations."
    )
    add("")
    add(
        "Criterion E will never appear here: it is a bespoke stochastic simulation per\n"
        "ecosystem, and the library accepts a collapse probability computed elsewhere."
    )
    add("")

    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify the page is up to date instead of writing it",
    )
    args = parser.parse_args()

    raw = TOML.read_bytes()
    table = tomllib.loads(raw.decode("utf-8"))
    digest = hashlib.sha256(raw).hexdigest()

    generated = render(table, digest)

    if args.check:
        current = OUTPUT.read_text() if OUTPUT.exists() else ""
        if current != generated:
            print(
                f"error: {OUTPUT.relative_to(ROOT)} is out of date.\n"
                f"       Run: python3 tools/generate_threshold_docs.py",
                file=sys.stderr,
            )
            return 1
        print(f"{OUTPUT.relative_to(ROOT)} is up to date")
        return 0

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(generated)
    print(f"wrote {OUTPUT.relative_to(ROOT)} (thresholds sha256 {digest[:12]})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
