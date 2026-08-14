---
title: IUCN Red List of Ecosystems
subtitle: Assessment calculations for Python, R, Julia, the browser and the command line
---

This library applies the [IUCN Red List of Ecosystems Categories and Criteria](https://portals.iucn.org/library/node/51533)
to the numbers an assessment produces, and tells you which risk category they imply —
including how uncertain that answer is.

It exists because two things were missing.

**Nothing assigned categories.** The canonical R package `redlistr` computes metrics but
stops there; `rle-python` implements the Criterion B thresholds and nothing else. The step
from "our EOO is 15,000 km²" to "this ecosystem is Endangered" was left to the assessor,
every time, by hand.

**Nothing expressed uncertainty.** Assessments routinely conclude something like
`EN (VU–CR)` — a best estimate with plausible bounds. The Guidelines ask for it explicitly.
No tool could represent it, so it was written into reports as prose and lost to any
downstream analysis.

## What an answer looks like

```text
Criterion B: EN (LC-EN)

  B1   EN (LC-EN)     (thresholds alone: EN)
  B2   EN (LC-EN)     (thresholds alone: EN)

  note: sub-conditions (a), (b), (c) not assessed, so the listing is provisional
```

That range is not hedging. Criterion B requires a spatial threshold **and** at least one of
three sub-conditions. When nobody has assessed those sub-conditions, the true answer really
does lie somewhere between Least Concern and Endangered, and reporting `EN` alone would
overstate what the evidence supports. See [the sub-condition gate](./concepts.md).

## Where to start

- **New to the Red List of Ecosystems?** [Understanding the criteria](./concepts.md)
  explains what the metrics measure, how the thresholds combine, and how to read a category
  range — with no code at all.
- **Ready to run something?** [Installation](./install.md), then
  [Quickstart](./quickstart.md), which shows the same assessment in Python, R, Julia,
  JavaScript, Rust and the command line.
- **Reviewing an assessment?** [Thresholds](./thresholds.md) is generated directly from the
  data file the library ships, so what you read there is what the engine applied.

## Status

Criterion B (restricted geographic distribution) is implemented and verified against the
2024 Guidelines. You supply the extent of occurrence, area of occupancy and sub-condition
evidence; the library applies the thresholds and the gate.

Computing EOO and AOO from distribution maps, and the remaining criteria, are in progress.
Criterion E will not be automated — it is a bespoke simulation per ecosystem, and the
library accepts a collapse probability you have computed elsewhere.

:::{note} On trusting this library
Every threshold is transcribed from the published Guidelines into a
[data file you can read](./thresholds.md), and a test asserts that the compiled code matches
that file. Three of the test cases are the Guidelines' own worked examples, so the engine is
checked against IUCN's arithmetic and not only against itself. Six language bindings run one
shared corpus of cases and must produce identical output.
:::
