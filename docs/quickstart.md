---
title: Quickstart
subtitle: The same assessment in six languages
---

Pick your language once — the tabs on this page stay in sync as you scroll.

Every example here mirrors a case in the shared conformance corpus, so the outputs are
values CI checks on each commit rather than numbers typed into a document.

## A first assessment

An ecosystem with an extent of occurrence of 15,000 km² and 15 occupied grid cells. No
sub-conditions have been assessed yet.

::::{tab-set}

:::{tab-item} Python
:sync: python

```python
import iucn_rle

result = iucn_rle.criterion_b(eoo_km2=15_000, aoo_cells=15)
result["overall"]
```
:::

:::{tab-item} R
:sync: r

```r
library(iucnrle)

result <- criterion_b(eoo_km2 = 15000, aoo_cells = 15)
result$overall
```
:::

:::{tab-item} Julia
:sync: julia

```julia
using JSON3

json = criterion_b("""{"eoo_km2": 15000, "aoo_cells": 15}""")
JSON3.read(json)["overall"]
```
:::

:::{tab-item} JavaScript
:sync: js

```js
import { criterionB } from '@rle-assessment/iucn-rle';

const result = criterionB({ eooKm2: 15000, aooCells: 15 });
result.overall;
```
:::

:::{tab-item} Rust
:sync: rust

```rust
use iucn_rle_core::{criterion_b, Basis, Estimate, Subconditions, ThresholdTable};

let eoo = Estimate::point(15_000.0, Basis::Estimated);
let aoo = Estimate::point(15.0, Basis::Estimated);

let assessment = criterion_b(Some(eoo), Some(aoo), &Subconditions::new(),
                             ThresholdTable::v2_2024())?;
assessment.category().to_string();
```
:::

:::{tab-item} Command line
:sync: cli

```bash
iucn-rle criterion-b --eoo-km2 15000 --aoo-cells 15
```
:::

::::

All six report:

```text
EN (LC-EN)
```

**Not a bare `EN`.** The metrics reach the Endangered thresholds, but Criterion B also needs
one of clauses (a), (b) or (c), and none has been assessed. If a clause turns out to hold
the answer is EN; if all three are ruled out it is LC. Both remain possible, so both are
reported. See [the sub-condition gate](./concepts.md).

## Establishing a clause

Say you have evidence of continuing decline in the ecosystem's spatial extent — clause (a).

::::{tab-set}

:::{tab-item} Python
:sync: python

```python
result = iucn_rle.criterion_b(
    eoo_km2=15_000,
    aoo_cells=15,
    clauses={"a": "met"},
)
result["overall"]        # 'EN'
```
:::

:::{tab-item} R
:sync: r

```r
result <- criterion_b(
  eoo_km2 = 15000,
  aoo_cells = 15,
  clauses = c(a = "met")
)
result$overall           # "EN"
```
:::

:::{tab-item} Julia
:sync: julia

```julia
json = criterion_b("""
  {"eoo_km2": 15000, "aoo_cells": 15,
   "clauses": [{"sub": "a", "status": "met"}]}
""")
JSON3.read(json)["overall"]   # "EN"
```
:::

:::{tab-item} JavaScript
:sync: js

```js
const result = criterionB({
  eooKm2: 15000,
  aooCells: 15,
  clauses: [{ sub: 'a', status: 'met' }],
});
result.overall;          // 'EN'
```
:::

:::{tab-item} Rust
:sync: rust

```rust
use iucn_rle_core::{ConditionStatus, DeclineAspect, Subconditions};

let subs = Subconditions::new()
    .with_decline(DeclineAspect::SpatialExtent, ConditionStatus::Met);

let assessment = criterion_b(Some(eoo), Some(aoo), &subs, ThresholdTable::v2_2024())?;
assessment.category().to_string();   // "EN"
```
:::

:::{tab-item} Command line
:sync: cli

```bash
iucn-rle criterion-b --eoo-km2 15000 --aoo-cells 15 --clause a=met
```
:::

::::

The range collapses to `EN`. Clause (a) is met if **any** of its three aspects is declining —
spatial extent, environmental quality, or biotic interactions — so you can be specific with
`a.i`, `a.ii` or `a.iii` where you know which.

## Clause (c) is a count, not a checkbox

This is the part most likely to catch you out. Clause (c) is a threshold on the number of
threat-defined locations, and **it differs by category**: exactly 1 for CR, ≤ 5 for EN,
≤ 10 for VU.

An ecosystem with an EOO of 1,500 km² — inside the CR band — three threat-defined locations,
and no decline or threatening processes:

::::{tab-set}

:::{tab-item} Python
:sync: python

```python
result = iucn_rle.criterion_b(
    eoo_km2=1_500,
    clauses={"a": "not_met", "b": "not_met"},
    locations=3,
)
result["criteria"][0]["category"]     # 'EN', not 'CR'
```
:::

:::{tab-item} R
:sync: r

```r
result <- criterion_b(
  eoo_km2 = 1500,
  clauses = c(a = "not_met", b = "not_met"),
  locations = 3
)
result$criteria[[1]]$category         # "EN", not "CR"
```
:::

:::{tab-item} Julia
:sync: julia

```julia
json = criterion_b("""
  {"eoo_km2": 1500, "locations": 3,
   "clauses": [{"sub": "a", "status": "not_met"},
               {"sub": "b", "status": "not_met"}]}
""")
JSON3.read(json)["criteria"][1]["category"]   # "EN", not "CR"
```
:::

:::{tab-item} JavaScript
:sync: js

```js
const result = criterionB({
  eooKm2: 1500,
  locations: 3,
  clauses: [
    { sub: 'a', status: 'not_met' },
    { sub: 'b', status: 'not_met' },
  ],
});
result.criteria[0].category;          // 'EN', not 'CR'
```
:::

:::{tab-item} Rust
:sync: rust

```rust
use iucn_rle_core::{CriterionId, ThreatLocations};

let subs = Subconditions::new()
    .with_decline(DeclineAspect::SpatialExtent, ConditionStatus::NotMet)
    .with_threatening_processes(ConditionStatus::NotMet)
    .with_locations(ThreatLocations::Count(3));

let assessment = criterion_b(Some(eoo), None, &subs, ThresholdTable::v2_2024())?;
assessment.result(CriterionId::B1).unwrap().category().best();   // Category::En
```
:::

:::{tab-item} Command line
:sync: cli

```bash
iucn-rle criterion-b --eoo-km2 1500 --clause a=not_met --clause b=not_met --locations 3
```

```text
Criterion B: EN

  B1   EN             (thresholds alone: CR)
```
:::

::::

The EOO alone says CR. But CR requires *exactly one* threat-defined location, and there are
three — so only the EN clause (≤ 5) is satisfied, and **EN is the correct listing**. The CLI
shows both, with `thresholds alone: CR` as the audit trail.

## No threats, versus threats you could not assess

These are different findings and the library keeps them apart.

::::{tab-set}

:::{tab-item} Python
:sync: python

```python
# A finding: threats were looked for and none exist.
iucn_rle.criterion_b(eoo_km2=1_500, no_plausible_threats=True)

# An absence: threats exist but their extent is unknown.
iucn_rle.criterion_b(eoo_km2=1_500, locations_insufficient_information=True)
```
:::

:::{tab-item} R
:sync: r

```r
# A finding: threats were looked for and none exist.
criterion_b(eoo_km2 = 1500, no_plausible_threats = TRUE)

# An absence: threats exist but their extent is unknown.
criterion_b(eoo_km2 = 1500, locations_insufficient_information = TRUE)
```
:::

:::{tab-item} Julia
:sync: julia

```julia
criterion_b("""{"eoo_km2": 1500, "no_plausible_threats": true}""")
criterion_b("""{"eoo_km2": 1500, "locations_insufficient_information": true}""")
```
:::

:::{tab-item} JavaScript
:sync: js

```js
criterionB({ eooKm2: 1500, noPlausibleThreats: true });
criterionB({ eooKm2: 1500, locationsInsufficientInformation: true });
```
:::

:::{tab-item} Rust
:sync: rust

```rust
Subconditions::new().with_locations(ThreatLocations::NoPlausibleThreats);
Subconditions::new().with_locations(ThreatLocations::InsufficientInformation);
```
:::

:::{tab-item} Command line
:sync: cli

```bash
iucn-rle criterion-b --eoo-km2 1500 --no-plausible-threats
iucn-rle criterion-b --eoo-km2 1500 --locations-insufficient-information
```
:::

::::

The first contributes to a finding of LC. The second yields **DD** — Data Deficient. Writing
the first where you mean the second turns "we did not look" into "it is fine", which is the
error this distinction exists to prevent.

## From a distribution map

Everything above assumed you already had EOO and AOO. You can also hand the library a
distribution map and let it compute them.

Coordinates are longitude/latitude degrees on WGS84. They are projected to the
equal-area grid internally, because a count of grid cells only means something if
every cell covers the same ground.

::::{tab-set}

:::{tab-item} Python
:sync: python

```python
square = [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]]

metrics = iucn_rle.distribution_metrics([("T1.1.1", square)])["ecosystems"][0]
metrics["eoo_km2"]               # 12308.46
metrics["aoo_cells"]             # 128  <- the Criterion B2 number
metrics["occupied_cell_count"]   # 144  <- cells touched, before the 1% exclusion
```
:::

:::{tab-item} R
:sync: r

```r
square <- list(list(c(0, 0), c(1, 0), c(1, 1), c(0, 1)))

metrics <- distribution_metrics(list(
  list(ecosystem = "T1.1.1", rings = square)
))
metrics$ecosystems[[1]]$eoo_km2
metrics$ecosystems[[1]]$aoo_cells
```
:::

:::{tab-item} Julia
:sync: julia

```julia
json = distribution_metrics("""
  [{"ecosystem": "T1.1.1",
    "rings": [[[0,0], [1,0], [1,1], [0,1]]]}]
""")
JSON3.read(json)["ecosystems"][1]["eoo_km2"]
```
:::

:::{tab-item} JavaScript
:sync: js

```js
const square = [[[0, 0], [1, 0], [1, 1], [0, 1]]];

const [metrics] = distributionMetrics([
  { ecosystem: 'T1.1.1', rings: square },
]).ecosystems;

metrics.eoo_km2;               // 12308.46
metrics.aoo_cells;             // 128  <- the Criterion B2 number
metrics.occupied_cell_count;   // 144  <- cells touched, before the 1% exclusion
```
:::

:::{tab-item} Rust
:sync: rust

```rust
use iucn_rle_core::distribution::DistributionAccumulator;

let mut acc = DistributionAccumulator::new();
acc.add_polygon("T1.1.1", &[vec![
    [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],
]]);

let distribution = acc.finish();
distribution.eoo_km2("T1.1.1");
distribution.aoo("T1.1.1").aoo_cells;
```
:::

:::{tab-item} Command line
:sync: cli

```bash
iucn-rle metrics distribution.geojson
```

```text
ECOSYSTEM         EOO (km2)        AOO   OCCUPIED
T1.1.1                 73.2          4          4

  grid ESRI:54034 at 10000 m
```

Reads a GeoJSON `FeatureCollection`; use `--ecosystem-property` to name the field
holding the ecosystem code, and `--format json` for machine-readable output.
:::

::::

Feed the result straight into `criterion_b` and you have an assessment from a map.

### Three things worth knowing

**Ring order decides what is a hole, not winding.** The first ring is the exterior and
the rest are holes, whichever direction each one winds. Source formats disagree —
GeoJSON specifies counter-clockwise exteriors, shapefiles the opposite, and real files
often follow neither — so the format cannot change your answer.

**Bad coordinates are rejected, not repaired.** A latitude of 200 raises an error
naming the ecosystem and the value. It almost always means the coordinates are swapped
or already projected, and quietly clamping would produce a plausible-looking but wrong
AOO.

**Overlapping features are reported.** If two polygons of the same ecosystem overlap,
the affected cells are capped at full occupancy and `overfull_cells` tells you how many.
That is a problem with the source map, and the library says so rather than absorbing it.

## Reading the result

Every surface returns the same structure:

| Field | What it holds |
|---|---|
| `overall` | The category, with bounds if uncertain: `"EN (LC-EN)"` |
| `overall_best` | The best estimate alone: `"EN"` |
| `is_threatened` | Whether the best estimate is CO, CR, EN or VU |
| `criteria[]` | Each sub-criterion: its category and its pre-gate `threshold_category` |
| `notes[]` | Machine-readable caveats, rendered for display |
| `guidelines_version` | Which edition of the criteria was applied |
| `thresholds_sha256` | Digest of the threshold table, for reproducibility |

`threshold_category` is the audit trail: what the metric alone implied, before the
sub-condition gate. When it differs from `category`, the gate changed the answer, and the
difference is exactly what a reviewer will want to see.

## Next

- [Understanding the criteria](./concepts.md) — what the metrics measure, and why ranges
  appear
- [Thresholds](./thresholds.md) — every number the engine applies, generated from the data
  file it ships
