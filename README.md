# rle-rust

IUCN Red List of Ecosystems assessment calculations, implemented in Rust and callable from
Python, R, the browser, and the command line.

> **Status: M0 — skeleton.** All four surfaces build and run, but the only function is
> `version()`. The Criterion B category engine lands in M1.

## Why

- **Memory.** The existing Python AOO grid computation peaks at many GB and OOM-kills CI
  runners on national datasets (Colombia: 460,350 features, 2.17 GB of geometry). A
  streaming implementation with bounded memory removes the need for the precomputed-cache
  workarounds built around that limit.
- **Coverage.** Only Criterion B is implemented in the current Python, and the canonical R
  package `redlistr` computes metrics but assigns no categories at all. Nothing in the
  ecosystem applies IUCN thresholds or represents plausible bounds such as `EN (VU–CR)`.
- **Reach.** Reading Cloud-Optimized GeoTIFFs over HTTP range requests needs no Earth
  Engine account, auth, or quota — on a laptop, in CI, or in a browser tab.

## Layout

```
crates/iucn-rle-core     pure, synchronous, no I/O — the domain. Compiles to wasm32 unchanged.
crates/iucn-rle-cli      the `iucn-rle` binary
bindings/python          pyo3 + maturin  ->  PyPI `iucn-rle`      (import iucn_rle)
bindings/r/iucnrle       extendr         ->  r-universe `iucnrle`
bindings/wasm            wasm-bindgen    ->  npm `@rle-assessment/iucn-rle`
```

`iucn-rle-core` is *sans-IO*: it never opens a socket, reads a file, or asks the clock for
the time. That is not a style preference. It is what lets the crate compile for
`wasm32-unknown-unknown`, and it makes the public API and the cross-language test surface
the same surface. Network code lives in `iucn-rle-io`, where async is used **only to fetch
bytes** — all parsing is synchronous over in-memory buffers.

`cargo check -p iucn-rle-core --target wasm32-unknown-unknown` runs in CI as a guard rail.
If it fails, a dependency that cannot target the browser has reached the core.

## Development

Requires a Rust toolchain ([rustup](https://rustup.rs)) and `just`:

```sh
pixi global install just     # or: brew install just, cargo install just
just --list                  # every recipe, with descriptions
```

```sh
just test          # cargo test --workspace
just lint          # fmt --check + clippy -D warnings
just deny          # licences, advisories, and the WASM-hostile crate ban
just wasm-check    # the guard rail
just versions      # print the version from all four bindings, to confirm they agree
just all           # build and run everything
```

Per-binding: `just python`, `just r`, `just wasm-node`, `just wasm-serve`.

Building a binding for the first time also needs its own toolchain — `maturin` for
Python (`uv tool install maturin`), `wasm-pack` for the browser
(`npm i -g wasm-pack`), and `rextendr` plus `devtools` for R.

## Relationship to other packages

Independent of [`rle-python`](https://github.com/RLE-Assessment/rle-python) — it does not
share the `rle` namespace. Numerical agreement with it is enforced by a shared conformance
corpus rather than by shared code: discrete outputs (occupied cells, AOO, categories) must
match exactly, and continuous outputs within `1e-9` relative tolerance.

`redlistr` is the validation oracle for the Criterion A decline maths, but **not** for AOO —
it grids from the input's own origin and jitters, whereas RLE assessments use a global
10 km grid snapped to (0, 0).

## License

Apache-2.0
