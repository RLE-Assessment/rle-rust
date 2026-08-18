# iucn-rle

IUCN Red List of Ecosystems assessment calculations, powered by Rust.

This package is **independent of `rle-python`** — it does not share the `rle` namespace.
The same calculation engine is available for [R](https://github.com/RLE-Assessment/rle-rust)
(`iucnrle`), the browser (`@rle-assessment/iucn-rle`), and as a standalone `iucn-rle` CLI,
and all four are held to a shared cross-language conformance corpus.

## Assess an ecosystem

```python
import iucn_rle

iucn_rle.criterion_b(eoo_km2=15_000, clauses={"a": "met"})["overall"]
# 'EN'
```

The spatial thresholds alone never produce a listing — Criterion B also requires one of
clauses (a), (b) or (c) — so metrics whose clauses nobody has examined come back as
`'EN (LC-EN)'` rather than a bare `'EN'` that would overstate what is known.

## Read a dataset without downloading it

```python
metrics = iucn_rle.distribution_metrics_from_url(
    "https://data.source.coop/tyler/colombia-ecosystems-map/"
    "ecosistemas/ECOSISTEMAS_MEC_122024.parquet",
    ecosystem_column="ecos_general",
)
```

GeoParquet keeps its metadata in a footer, so the read fetches that first, uses each row
group's statistics to rule out the ones that cannot match, then streams the survivors one
at a time. Colombia's national map is 1.81 GB; reading all 460,350 features of it peaks at
**221 MB of memory**, and reading only its structure costs 382 KB — 0.02% of the file.

The `read` key reports what the read actually cost, so the claim is checkable rather than
asserted:

```python
metrics["read"]
# {'row_groups_read': 1, 'row_groups_skipped': 0, 'features': 4944,
#  'bytes_fetched': 16227391, 'crs': 'EPSG:4686'}
```

Pass `bbox=` or `ecosystems=` to read less. Note the URL must be the **file**, not a portal
page describing it; the two often differ only by hostname, and this is the single most
common way a read fails, so the error names the corrected address when it can recognise one.

## Concurrency

The API is synchronous. Every long-running function releases the GIL for the whole
computation — including the network wait — so a Jupyter or Quarto kernel stays responsive
using nothing but the standard library:

```python
metrics = await asyncio.to_thread(
    iucn_rle.distribution_metrics_from_url, url, "ecos_general"
)
```

This is a contract rather than an optimisation, and it is invisible in the returned
values, so the test suite verifies it by watching whether the main thread keeps running
during a call rather than by inspecting output.

## Status

Pre-alpha. Criterion B and its spatial metrics are implemented, from local geometry or a
remote GeoParquet URL. Criteria A, C, D and E are not yet.

## License

Apache-2.0
