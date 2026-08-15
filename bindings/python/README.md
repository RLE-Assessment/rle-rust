# iucn-rle

IUCN Red List of Ecosystems assessment calculations, powered by Rust.

```python
import iucn_rle

iucn_rle.version()
```

This package is **independent of `rle-python`** — it does not share the `rle` namespace.
The same calculation engine is available for [R](https://github.com/RLE-Assessment/rle-rust)
(`iucnrle`), the browser (`@rle-assessment/iucn-rle`), and as a standalone `iucn-rle` CLI,
and all four are held to a shared cross-language conformance corpus.

## Concurrency

The API is synchronous. Long-running functions release the GIL for the whole
computation, so a Jupyter or Quarto kernel stays responsive using only the standard
library:

```python
grid = await asyncio.to_thread(iucn_rle.aoo_grid, url)
```

## Status

Pre-alpha (M0). Only `version()` exists; the Criterion B category engine lands in M1.

## License

Apache-2.0
