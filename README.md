# rle-rust

IUCN Red List of Ecosystems assessment calculations, implemented in Rust and callable from
Python, R, the browser, and the command line.

> **Status: M1.** Criterion B works end to end on every surface. No geometry and no
> network yet — you supply EOO and AOO, the engine applies the IUCN thresholds and the
> sub-condition gate. Spatial computation lands in M2, remote data in M3.

```sh
$ iucn-rle criterion-b --eoo-km2 15000 --aoo-cells 15
Criterion B: EN (LC-EN)

  B1   EN (LC-EN)     (thresholds alone: EN)
  B2   EN (LC-EN)     (thresholds alone: EN)

  note: sub-conditions (a), (b), (c) not assessed, so the listing is provisional
```

That range is the point. Criterion B needs a spatial threshold **and** at least one of
clause (a) continuing decline, (b) threatening processes, or (c) few threat-defined
locations. `rle-python` notes this in a docstring; `redlistr` assigns no categories at
all. Here it is typed, so "we have not checked" produces an honest range instead of a
bare `EN` that overstates confidence — and the Guidelines ask for exactly this, at
§6.3.2 p. 70: *"upper and lower bounds of the status under criterion B should be
estimated by propagating both scenarios through the criteria."* Say `--clause a=met` and
you get `EN`.

**Clause (c) is a count, not a checkbox**, and it is category dependent — 1 threat-defined
location for CR, ≤ 5 for EN, ≤ 10 for VU. So evaluation runs per level:

```sh
$ iucn-rle criterion-b --eoo-km2 1500 --clause a=not_met --clause b=not_met --locations 3
Criterion B: EN

  B1   EN             (thresholds alone: CR)
```

An EOO of 1,500 km² is in the CR band, but CR requires exactly one location. With three,
only the EN clause is satisfied. Treating (c) as a boolean would report CR and overstate
the threat by a full category.

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
crates/iucn-rle-capi     extern "C" ABI  ->  Julia (ccall) and any future language
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
just conformance   # run the shared corpus through every surface
just all           # the full gate
```

Per-binding: `just python`, `just r`, `just wasm-node`, `just wasm-serve`.

## Conformance

`fixtures/cases/*.json` holds the cross-language corpus, and Rust, the C ABI, Python, R,
and JavaScript each run the identical file. Categories are compared as display strings,
so `"EN (LC-EN)"` is one exact comparison in every language with no float tolerance to
negotiate. A binding is not finished until it passes.

Three cases come from the Guidelines' own published worked examples — Great Fish Thicket
(Box 12, p. 69), Cape Flats Sand Fynbos and Coolibah-Black Box Woodland (Box 14, p. 75) —
so the engine is checked against IUCN's own arithmetic, not only against itself.

That is what makes "all the bindings agree" checkable rather than aspirational:

```sh
just conformance
```

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
