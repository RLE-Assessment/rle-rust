---
title: Reading remote data
short_title: Remote data
---

How this library reads a dataset it does not have, and what real files taught us about
doing it. Written down because most of it was discovered rather than designed, and every
item cost something to find.

## Reading one

Python is the first binding to expose this. Nothing is written to disk.

```python
import iucn_rle

metrics = iucn_rle.distribution_metrics_from_url(
    "https://data.source.coop/tyler/colombia-ecosystems-map/"
    "ecosistemas/ECOSISTEMAS_MEC_122024.parquet",
    ecosystem_column="ecos_general",
)
metrics["read"]
# {'row_groups_read': 47, 'row_groups_skipped': 0, 'features': 460350,
#  'bytes_fetched': 1792600000, 'crs': 'EPSG:4686'}
```

The result is what a local `distribution_metrics` returns plus that `read` key, so
anything downstream takes either. `bbox=` and `ecosystems=` restrict what is read.

The call **releases the GIL** for the whole fetch and decode, which matters because
Quarto and Jupyter kernels already run an asyncio event loop that a blocking call would
freeze for the duration:

```python
metrics = await asyncio.to_thread(
    iucn_rle.distribution_metrics_from_url, url, "ecos_general"
)
```

That is a contract rather than an optimisation, and it is invisible in the returned
numbers — identical either way — so it is tested by watching whether the main thread
keeps running during a call. Without the release it ran 0 times in 0.59 s; with it, 376.

To read a file's structure without assessing it, `just inspect <url>` fetches the footer
only and reports the schema, row groups, CRS, GeoParquet version and any structural
problems.

## The shape of it

Three layers, and the split is the design:

| Crate | Does | Async? |
|---|---|---|
| `iucn-rle-io` | Fetches byte ranges. Nothing else. | Yes — the only async in the project |
| `iucn-rle-format` | Decodes bytes already in memory | No |
| `iucn-rle-engine` | The loop between them | Yes |

Keeping decoding synchronous is what keeps `Send` bounds out of the parsers and lets the
whole stack target `wasm32-unknown-unknown`, where futures are not `Send`.

The engine's loop is the deliverable: **prune, then stream**. Decide from the footer's
statistics which row groups can possibly match, fetch one, decode it, fold it into an
accumulator, and drop it before fetching the next. Fetching everything a plan names is
simpler, gives identical answers, and puts the whole selection in memory — which is the
failure this exists to fix. `ReadReport` records what actually crossed the wire so the
claim is checkable rather than asserted.

## What it costs on a national dataset

Colombia's ecosystems map, 1.81 GB, read over HTTP:

```text
460,350 features · 47 row groups · 87 ecosystems
1792.6 MB fetched · 164s · 221.0 MB peak
```

Reading only its *structure* — row count, row groups, CRS, 50-column schema — costs
**382 KB, 0.021% of the file**. That is what `just inspect <url>` does.

Peak memory was 941 MB before features were streamed one at a time rather than
materialised per row group; same file, identical answers, 4.3× less memory.

## What real files taught us

Every one of these was found by pointing the reader at published data. None would have
been caught by a fixture, and none by schema validation.

**A national CRS is not EPSG:4326.** Colombia's map is EPSG:4686 (MAGNA-SIRGAS) —
geographic, in degrees, and not 4326. An allow-list of one code refuses the very files
this exists to read. The requirement is "longitude/latitude in degrees", which PROJJSON
states directly as a CRS `type` and axis units.

**Real GeoParquet is ZSTD-compressed**, and ZSTD is the one common codec that cannot
reach the browser, because it binds a C library. Native enables it; the WASM build keeps
every pure-Rust codec and reports the gap by name.

**`reqwest`'s `content_length()` is the body's length**, and a HEAD response has none —
so it answers 0 for every object. `HttpSource` had only ever been tested against
in-memory sources; it now has a hand-rolled test server that can also misbehave.

**Declared versions are not reliable.** geopandas 1.1.4 writes `"version": "1.0.0"`
while using the `covering` key introduced in 1.1. That is legal — the schema permits
unknown keys — so a reader that gated features on the declared version would silently
lose all spatial pruning on files from the most widely used writer there is.

**GeoParquet 2.0 moves geometry into Parquet itself**, as the native `GEOMETRY` logical
type. Files using it need parquet ≥ 57; 56 rejects them outright. Such files now open,
but decoding them is *unproven* — nothing here handles that logical type deliberately.

## Two kinds of checking, neither sufficient alone

- **Schema validation** (`schema-validation` feature) says the `geo` metadata is
  well-formed per the published specification.
- **Structural checks** (always available) say the metadata describes the file it is
  actually in: that `primary_column` names a real column, that covering columns exist,
  that the CRS is usable.

Colombia's map passes the first and failed the second four different ways. Both are
advisory: a technically imperfect file is usually still readable, and refusing it would
block real work for nothing.

## A remote read is made of round trips, not bytes

Worth stating because it is the opposite of the intuition the rest of this page builds.
Everything above is about transferring *less*, and on a national file that is right. On
anything smaller, the read is dominated by how many times it waits.

Measured on the Bogotá subset — 16 MB, one row group:

| | |
|---|---|
| Moving the data | 0.70 s |
| Decoding it | 0.11 s |
| Actual read | 1.39 s |

The difference is four **sequential** requests: a HEAD to learn the size, a GET for the
footer, then the row group's column chunks one after another. Serving the same file over
loopback with a fixed 0.1 s per request costs 0.53 s against 0.11 s, which is how the
count was established rather than guessed.

Two changes remove half of them, and neither needs more bandwidth:

* **A suffix range request** (`Range: bytes=-65536`) returns the footer *and* the total
  size in `Content-Range`. The HEAD existed only to work out where to ask from.
* **Issuing a row group's column ranges together** rather than in sequence, with
  `buffered()` on the same current-thread runtime.

Four round trips become two: 0.54 s to 0.33 s at 0.1 s of latency, same answers.

This also explains why the obvious optimisation is the wrong one. Overlapping the decode
with the fetch, on a worker thread, was measured at about 7% because decode is 0.11 s of
1.39 s — and it would double peak row-group memory to get it. Bandwidth is not the
constraint either: fetching the file as four parallel ranges took 0.61 s against 0.70 s
sequentially. **Count the waits before optimising the work.**

## Guarantees, and where they stop

|  | bounds memory | bounds bytes transferred |
|---|---|---|
| GeoParquet | yes | yes — row-group statistics allow pruning |
| COG | yes | **no** — a COG carries no per-tile statistics |

A raster reader cannot know a tile is irrelevant without decoding it, so the raster path
gives a strictly weaker guarantee than the vector one.

Spatial pruning also needs bbox covering columns, which GeoParquet 1.0 files do not
have — Colombia's included. Without them every row group must be read. Writing them
makes regional queries dramatically cheaper, and is worth doing when a file is next
produced.

## Gotchas

Data portals often serve a browsable interface and the bytes themselves from different
hosts at identical paths. `source.coop/...` returns HTML; `data.source.coop/...` returns
the file. The tools detect this specific case and offer the corrected URL.

A server must honour range requests. Python's `http.server` does not, so it cannot be
used to serve a fixture locally for testing.
