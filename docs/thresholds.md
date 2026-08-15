---
title: Thresholds
subtitle: Generated from the data file the library ships
---

:::{important} This page is generated
Every number below is read directly from
`crates/iucn-rle-core/thresholds/iucn-rle-v2.0-2024.toml`, the same file the
engine compiles in. A test asserts the compiled constants match that file, and
CI fails if this page falls out of step with it. What you read here is what the
library applied.
:::

**Source.** IUCN (2024) Guidelines for the application of IUCN Red List of Ecosystems Categories and Criteria, Version 2.0

- Guidelines version **2.0** (2024)
- Criteria version **2.1**
- Threshold table SHA-256: `31270e2253956e318b9c611b466e2d0a0b8de3ebe9bfbaaf014173b9cb43a06a`

The digest is recorded in the provenance of every result, so a category can be
traced back to the exact numbers that produced it.

## B1

*Area of a minimum convex polygon enclosing all occurrences (extent of occurrence, EOO), in square kilometres*

Source: Appendix 1, p. 154; Section 6.2, pp. 65-66.

| Category | Metric threshold |
|---|---|
| **CR** Critically Endangered | ≤ 2,000 |
| **EN** Endangered | ≤ 20,000 |
| **VU** Vulnerable | ≤ 50,000 |
| **LC** Least Concern | above every threshold |

Bounds are inclusive: a value equal to a bound meets that category.

### Clause (c): threat-defined locations

| Category | Clause is met at |
|---|---|
| **CR** Critically Endangered | exactly 1 location |
| **EN** Endangered | ≤ 5 locations |
| **VU** Vulnerable | ≤ 10 locations |

This bound is **category dependent**, which is why the library
evaluates each category level separately rather than deriving a
category from the metric and then applying a yes/no gate.

A listing under B1 requires the metric threshold **and** at least one
of clauses (a), (b), (c).

B1 never yields: **NT** Near Threatened.

## B2

*Number of occupied 10 x 10 km grid cells (area of occupancy, AOO), after the 1% small-patch exclusion*

Source: Appendix 1, p. 155; Section 6.2, pp. 65-66.

| Category | Metric threshold |
|---|---|
| **CR** Critically Endangered | ≤ 2 |
| **EN** Endangered | ≤ 20 |
| **VU** Vulnerable | ≤ 50 |
| **LC** Least Concern | above every threshold |

Bounds are inclusive: a value equal to a bound meets that category.

### Clause (c): threat-defined locations

| Category | Clause is met at |
|---|---|
| **CR** Critically Endangered | exactly 1 location |
| **EN** Endangered | ≤ 5 locations |
| **VU** Vulnerable | ≤ 10 locations |

This bound is **category dependent**, which is why the library
evaluates each category level separately rather than deriving a
category from the metric and then applying a yes/no gate.

A listing under B2 requires the metric threshold **and** at least one
of clauses (a), (b), (c).

B2 never yields: **NT** Near Threatened.

## B3

*No spatial metric; assessed from threat-defined locations and capability of rapid collapse*

Source: Appendix 1, p. 155; Section 6.3.3, p. 75.

This sub-criterion has no spatial metric; the outcome rests entirely on
the sub-conditions below.

### Threat-defined locations

| Category | Clause is met at |
|---|---|
| **VU** Vulnerable | ≤ 4 locations |

B3 never yields: **NT** Near Threatened, **CR** Critically Endangered, **EN** Endangered.

## Not yet transcribed

Criteria A, C, D and E have no table here. Their thresholds are published in
Appendix 1 of the Guidelines, but transcribing numbers that no code exercises
and no test checks is how an unverified value gets in. They arrive with their
own implementations.

Criterion E will never appear here: it is a bespoke stochastic simulation per
ecosystem, and the library accepts a collapse probability computed elsewhere.
