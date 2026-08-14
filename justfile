# One entry point, so contributors never have to memorise four toolchains.
# `just` docs: https://github.com/casey/just

root := justfile_directory()

default:
    @just --list

# --- Rust -------------------------------------------------------------------

build:
    cargo build --workspace

test:
    cargo test --workspace

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

fmt:
    cargo fmt --all

# The guard rail. If this fails, a non-WASM dependency reached the core crate and
# the browser binding is broken — fix the dependency, never this command.
wasm-check:
    cargo check -p iucn-rle-core --target wasm32-unknown-unknown

# --- Bindings ---------------------------------------------------------------

python:
    maturin build -m {{root}}/bindings/python/Cargo.toml --out {{root}}/target/wheels

python-dev:
    maturin develop -m {{root}}/bindings/python/Cargo.toml

wasm:
    wasm-pack build {{root}}/bindings/wasm --target bundler \
        --out-dir {{root}}/target/wasm-bundler --out-name iucn_rle

wasm-node:
    wasm-pack build {{root}}/bindings/wasm --target nodejs \
        --out-dir {{root}}/target/wasm-node --out-name iucn_rle

r:
    Rscript -e 'setwd("bindings/r/iucnrle"); devtools::document(); devtools::install()'

r-check:
    Rscript -e 'setwd("bindings/r/iucnrle"); devtools::check()'

# --- Everything -------------------------------------------------------------

# Prove all four surfaces still agree. This is what M0 exists to keep green.
all: lint test wasm-check python wasm-node r
    @echo "All four surfaces built."
