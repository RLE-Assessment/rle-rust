# One entry point, so contributors never have to memorise four toolchains.
# `just` docs: https://github.com/casey/just
#
# NOTE: only the LAST comment line before a recipe becomes its `just --list`
# description, so keep those to a single line and put rationale in the body.

root := justfile_directory()

# Show all available recipes.
default:
    @just --list

# --- Rust -------------------------------------------------------------------

# Build every crate in the workspace.
build:
    cargo build --workspace

# Run the Rust test suite.
test:
    cargo test --workspace

# Check formatting and lint with warnings as errors.
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

# Apply rustfmt across the workspace.
fmt:
    cargo fmt --all

# Verify dependency licences, advisories, and the WASM-hostile crate ban.
deny:
    cargo deny check

# If this fails, a dependency that cannot target wasm32 has reached the core
# crate and the browser binding is broken. Fix the dependency, never this recipe.

# Guard rail: iucn-rle-core must stay compilable for the browser.
wasm-check:
    cargo check -p iucn-rle-core --target wasm32-unknown-unknown

# --- Bindings ---------------------------------------------------------------

# Build the Python wheel into target/wheels.
python:
    maturin build -m {{root}}/bindings/python/Cargo.toml --out {{root}}/target/wheels

# Rebuild and install the Python binding into the active virtualenv.
python-dev:
    maturin develop -m {{root}}/bindings/python/Cargo.toml

# Build the WASM package for bundlers (webpack, vite).
wasm:
    wasm-pack build {{root}}/bindings/wasm --target bundler \
        --out-dir {{root}}/target/wasm-bundler --out-name iucn_rle

# Build the WASM package for Node.
wasm-node:
    wasm-pack build {{root}}/bindings/wasm --target nodejs \
        --out-dir {{root}}/target/wasm-node --out-name iucn_rle

# Build the WASM package for the browser, loadable as an ES module.
wasm-web:
    wasm-pack build {{root}}/bindings/wasm --target web \
        --out-dir {{root}}/target/wasm-web --out-name iucn_rle

# Serve the browser WASM build at http://localhost:8000.
wasm-serve: wasm-web
    python3 -m http.server 8000 --directory {{root}}/target/wasm-web

# Recompile the Rust, regenerate wrappers, and install the R package.
r:
    Rscript -e 'setwd("{{root}}/bindings/r/iucnrle"); rextendr::document(); devtools::install()'

# Run R CMD check on the R package.
r-check:
    Rscript -e 'setwd("{{root}}/bindings/r/iucnrle"); devtools::check()'

# --- Everything -------------------------------------------------------------

# Build and run all four surfaces. This is what M0 exists to keep green.
all: lint test deny wasm-check python wasm-node r
    @echo "All four surfaces built."

# Print the version from every binding, to confirm they agree.
versions:
    @printf 'cli    '; cargo run -q -p iucn-rle-cli -- version
    @printf 'python '; {{root}}/target/venv/bin/python -c "import iucn_rle; print(iucn_rle.version())"
    @printf 'r      '; Rscript -e 'setwd("{{root}}/bindings/r/iucnrle"); pkgload::load_all(quiet=TRUE); cat(rle_version(), "\n")'
    @printf 'wasm   '; node -e "console.log(require('{{root}}/target/wasm-node/iucn_rle.js').version())"
