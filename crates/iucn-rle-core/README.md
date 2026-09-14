# iucn-rle-core

IUCN Red List of Ecosystems assessment calculations: the categories, the thresholds, and
the spatial metrics they are applied to. Pure, synchronous, and free of I/O.

This is the engine behind the [`iucn-rle`](https://pypi.org/project/iucn-rle/) Python
package, the `iucnrle` R package, `@rle-assessment/iucn-rle` on npm, and the `iucn-rle`
command line tool. All of them call the same code and are held to one shared conformance
corpus, so an assessment does not depend on the language it was run from.

## Assess an ecosystem

```rust
use iucn_rle_core::{criterion_b, Basis, Estimate, Subconditions, ThresholdTable};

let eoo = Estimate::point(15_000.0, Basis::Estimated);
let assessment = criterion_b(Some(eoo), None, &Subconditions::new(), ThresholdTable::v2_2024())?;

assert_eq!(assessment.category().to_string(), "EN (LC-EN)");
# Ok::<(), iucn_rle_core::NoThresholdTable>(())
```

**That range is the point, and it is why this crate exists.** An extent of occurrence of
15,000 km² is inside the Endangered band — but Criterion B lists an ecosystem only if a
spatial threshold is met **and** at least one of clause (a) continuing decline, (b)
threatening processes, or (c) few threat-defined locations. Nobody has examined the
clauses here, so the honest answer spans everything still possible: `LC` if all three are
ruled out, `EN` if any holds.

The Guidelines ask for exactly this (§6.3.2, p. 70): *"upper and lower bounds of the
status under criterion B should be estimated by propagating both scenarios through the
criteria."* Establish a clause and the range collapses:

```rust
# use iucn_rle_core::{criterion_b, Basis, ConditionStatus, DeclineAspect, Estimate, Subconditions, ThresholdTable};
let subs = Subconditions::new()
    .with_decline(DeclineAspect::SpatialExtent, ConditionStatus::Met);

let eoo = Estimate::point(15_000.0, Basis::Estimated);
let assessment = criterion_b(Some(eoo), None, &subs, ThresholdTable::v2_2024())?;

assert_eq!(assessment.category().to_string(), "EN");
# Ok::<(), iucn_rle_core::NoThresholdTable>(())
```

`ConditionStatus` has three values, not two: `Met`, `NotMet`, and `NotAssessed`. "We did
not look" and "we looked and it is not happening" are different findings, and collapsing
them is how a provisional listing becomes a confident one by accident.

## Clause (c) is a count, not a checkbox

It is a threshold on the number of threat-defined locations, and it **differs by
category**: exactly 1 for CR, ≤ 5 for EN, ≤ 10 for VU. So evaluation runs per level
rather than picking a category from the metric and then applying a boolean gate.

```rust
# use iucn_rle_core::{criterion_b, Basis, ConditionStatus, CriterionId, DeclineAspect, Estimate, Subconditions, ThreatLocations, ThresholdTable};
// Clause (a) holds if *any* of its three aspects does, so ruling it out means ruling
// out all three. Leave one unassessed and the answer stays a range, correctly.
let mut subs = Subconditions::new()
    .with_threatening_processes(ConditionStatus::NotMet)
    .with_locations(ThreatLocations::Count(3));
for aspect in DeclineAspect::ALL {
    subs = subs.with_decline(aspect, ConditionStatus::NotMet);
}

let eoo = Estimate::point(1_500.0, Basis::Estimated);
let assessment = criterion_b(Some(eoo), None, &subs, ThresholdTable::v2_2024())?;
let b1 = assessment.result(CriterionId::B1).unwrap();

assert_eq!(b1.category().to_string(), "EN");
assert_eq!(b1.threshold_category().unwrap().to_string(), "CR");
# Ok::<(), iucn_rle_core::NoThresholdTable>(())
```

1,500 km² is in the CR band, but CR requires exactly one threat-defined location and
there are three — so only the EN clause is satisfied. Treating (c) as a boolean reports
CR and overstates the threat by a full category. Every result keeps the pre-gate
`threshold_category` beside the final one, which is the audit trail: when they differ,
the gate changed the answer.

That comment about clause (a) is not pedantry. Rule out only the spatial-extent aspect
and this same case returns `CR (EN-CR)`, because an unexamined aspect could still make
(a) hold, and (a) holding would qualify CR. The engine will not narrow a range on
evidence nobody gathered.

## Metrics from a distribution map

Hand it polygons in longitude/latitude degrees and it computes both Criterion B metrics.
Features are folded in one at a time and dropped, so memory tracks *occupied grid cells*
rather than feature count — a national map does not have to fit in RAM.

```rust
use iucn_rle_core::distribution::DistributionAccumulator;

let mut accumulator = DistributionAccumulator::new();
accumulator.add_polygon("T1.1.1", &[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]]);

let distribution = accumulator.finish();
let aoo = distribution.aoo("T1.1.1");

assert!(distribution.eoo_km2("T1.1.1") > 12_000.0);
assert_eq!(aoo.aoo_cells, 128);            // the Criterion B2 metric
assert_eq!(aoo.occupied_cell_count, 144);  // cells touched, before the 1% exclusion
```

Those last two are different numbers and only the first is B2 — the 1% rule drops cells
holding a negligible sliver of the ecosystem. Conflating them is an easy way to report an
AOO that is too high.

Two more behaviours worth knowing. A ring's role is decided by its **position**, not its
winding: `rings[0]` is the exterior and the rest are holes, whichever way each one winds,
because source formats disagree and the format must not change an assessment. And
out-of-range coordinates are **rejected rather than clamped** — a latitude of 200 almost
always means the values are swapped or already projected, and coping quietly would
produce a plausible but wrong answer.

## Thresholds are data, not code

The numbers live in a versioned TOML file that ships with the crate, and a build script
hashes it. A test binds the parsed file to the compiled constants, so the table cannot
drift from the code that applies it, and every assessment carries the digest in its
provenance record.

```rust
let table = iucn_rle_core::ThresholdTable::v2_2024();
assert_eq!(table.guidelines_version(), "2.0");
```

Semantics are verified against IUCN (2024) *Guidelines for the application of IUCN Red
List of Ecosystems Categories and Criteria, Version 2.0*, §6 and Appendix 1 (criteria
version 2.1): inclusive upper bounds, first match wins, above every bound is `LC`, an
absent metric is not evaluated.

## Sans-IO, and why it matters

This crate never opens a socket, reads a file, or asks the clock for the time. That is a
constraint rather than a style, and two things follow from it:

- It compiles for `wasm32-unknown-unknown` unchanged, which is what makes the browser
  binding possible. CI runs `cargo check --target wasm32-unknown-unknown` as a guard rail.
- Its public API and its test surface are the same surface, so one fixture corpus can
  exercise all of it from Rust, Python, R, and JavaScript.

Reading remote `GeoParquet` and Cloud-Optimized `GeoTIFF` lives in `iucn-rle-io` and
`iucn-rle-engine`, where async is used **only to fetch bytes** — every decoder is
synchronous over buffers already in memory.

## Status

Criterion B and its spatial metrics are implemented and cross-checked against the
Guidelines' own published worked examples. Criteria A, C, D and E are not yet. The API is
pre-1.0 and will change.

## License

Apache-2.0. Source, issues, and the other language bindings:
<https://github.com/RLE-Assessment/rle-rust>
