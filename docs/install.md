---
title: Installation
subtitle: Six ways in, one engine underneath
---

Every surface below calls the same Rust core, so they produce identical answers by
construction — and a shared corpus of test cases proves it on every commit. Pick whichever
fits your workflow.

:::{warning} Pre-release
Nothing is published to a package registry yet. The instructions below are what installation
will look like; today, build from a checkout — see [](#from-source).
:::

::::{tab-set}

:::{tab-item} Python
:sync: python

```bash
pip install iucn-rle          # or: uv add iucn-rle
```

Requires Python 3.11 or newer. Wheels are `abi3`, so one wheel per platform covers every
supported interpreter version.

```python
import iucn_rle
iucn_rle.version()
```

This package is **independent of `rle-python`** and does not share the `rle` namespace, so
the two can be installed side by side.
:::

:::{tab-item} R
:sync: r

```r
install.packages("iucnrle", repos = "https://rle-assessment.r-universe.dev")
```

```r
library(iucnrle)
engine_version()
```

Published via r-universe first. CRAN requires vendored Rust sources and has strict rules
about compiled dependencies; that is a later goal.
:::

:::{tab-item} Julia
:sync: julia

:::{note} Experimental
There is no `IucnRle.jl` package yet. Until there is, call the C ABI directly with `ccall`.
The interface below is stable — it is tested on every commit — but the ergonomic Julia
wrapper is still to come.
:::

Build the shared library from a checkout, then load it:

```julia
const LIB = "target/release/libiucn_rle"

function criterion_b(args::String)
    ptr = ccall((:iucn_rle_criterion_b, LIB), Cstring, (Cstring,), args)
    result = unsafe_string(ptr)
    ccall((:iucn_rle_string_free, LIB), Cvoid, (Cstring,), ptr)
    return result
end
```

Arguments and results both cross as JSON. Every pointer the library returns is owned by you
and must be released with `iucn_rle_string_free`.
:::

:::{tab-item} JavaScript
:sync: js

```bash
npm install @rle-assessment/iucn-rle
```

Works in Node and in the browser. Nothing is fetched at runtime and there is no native
dependency — the whole engine compiles to WebAssembly.

```js
import init, { version } from '@rle-assessment/iucn-rle';
await init();          // browser only; not needed under Node
version();
```
:::

:::{tab-item} Rust
:sync: rust

```bash
cargo add iucn-rle-core
```

The core crate is *sans-IO*: no network, no filesystem, no clock. It compiles for
`wasm32-unknown-unknown` unchanged, which is what makes the browser build possible.

```rust
iucn_rle_core::version();
```
:::

:::{tab-item} Command line
:sync: cli

```bash
cargo install iucn-rle-cli
```

```bash
iucn-rle version
iucn-rle --help
```

Useful in CI and in shell pipelines: `--format json` emits machine-readable output from
every subcommand.
:::

::::

(from-source)=
## From source

You need a [Rust toolchain](https://rustup.rs) and [`just`](https://github.com/casey/just):

```bash
git clone https://github.com/RLE-Assessment/rle-rust
cd rle-rust
just --list
```

Each binding also needs its own build tool — `maturin` for Python
(`uv tool install maturin`), `wasm-pack` for JavaScript (`npm i -g wasm-pack`), and
`rextendr` plus `devtools` for R.

```bash
just python      # build the Python wheel
just r           # build and install the R package
just wasm-node   # build the WASM package for Node
just build       # the Rust crates and the CLI
```

To check that every surface agrees before you rely on it:

```bash
just conformance
```

That runs one shared corpus of cases through Rust, the C ABI, Python, R and JavaScript, and
fails if any of them disagrees.

## Which surface should I use?

- **Python or R** if you are writing an assessment. Both integrate with the spatial stacks
  you already use, and both return native data structures.
- **The command line** for CI, batch runs, or a quick check without a project.
- **JavaScript** for interactive tools and browser-based viewers, where there is no server
  and no install.
- **Rust** if you are embedding the engine in something larger.
- **The C ABI** for Julia today, and for any language not listed here.
