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

**Criterion B works end to end.** Hand it a distribution map and it computes the extent of
occurrence and area of occupancy, applies the IUCN thresholds, and applies the sub-condition
gate. Or supply EOO and AOO from elsewhere and it does the classification alone.

Criteria A, C and D are in progress. Criterion E will not be automated — it is a bespoke
simulation per ecosystem, and the library accepts a collapse probability you computed
elsewhere.

Reading distribution maps directly from remote URLs, without downloading them first, is the
next milestone.

:::{note} On trusting this library
Every threshold is transcribed from the published Guidelines into a
[data file you can read](./thresholds.md), and a test asserts that the compiled code matches
that file. Three test cases are the Guidelines' own worked examples, so the engine is checked
against IUCN's arithmetic and not only against itself.

The spatial calculations are checked against `rle-python`, the implementation assessments
currently use. On its own committed test dataset, the occupied-cell sets and cell counts
match **exactly**, and the per-cell extents agree to **1.2 × 10⁻¹³**. The extent of occurrence
reproduces the value published in the RLE workshop material to the precision it was published
at. The projection matches PROJ to under **2 nanometres**.

Six language bindings run one shared corpus and must produce identical output.
:::
