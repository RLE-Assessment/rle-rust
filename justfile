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

# --- Conformance ------------------------------------------------------------
#
# One corpus (fixtures/cases/*.json), five runners. This is what makes "all the
# bindings agree" a checkable claim rather than an aspiration.

# Run the conformance corpus through Rust and the C ABI.
conformance-rust:
    cargo test -p iucn-rle-core --test conformance
    cargo test -p iucn-rle-capi

# Run the conformance corpus through the Python binding.
conformance-python: python
    uv pip install --python {{root}}/target/venv/bin/python -q --force-reinstall \
        {{root}}/target/wheels/*.whl
    {{root}}/target/venv/bin/python -m pytest {{root}}/bindings/python/tests -q

# Run the conformance corpus through the WASM binding under Node.
conformance-js: wasm-node
    node --test {{root}}/js/conformance.mjs {{root}}/js/golden.mjs

# Run the conformance corpus through the R binding.
conformance-r:
    Rscript -e 'setwd("{{root}}/bindings/r/iucnrle"); pkgload::load_all(quiet=TRUE); \
        testthat::test_dir("tests/testthat", package="iucnrle", load_package="none", \
        stop_on_failure=TRUE)'

# Run the conformance corpus through every surface. If this passes, they agree.
conformance: conformance-rust conformance-python conformance-js conformance-r
    @echo "All surfaces agree on the conformance corpus."

# --- Documentation ----------------------------------------------------------

# Regenerate docs/thresholds.md from the threshold table the library ships.
docs-thresholds:
    python3 {{root}}/tools/generate_threshold_docs.py

# Fail if docs/thresholds.md has drifted from the threshold table.
docs-thresholds-check:
    python3 {{root}}/tools/generate_threshold_docs.py --check

# Regenerate the ESRI:54034 reference fixture from PROJ. Needs pyproj.
projection-fixture:
    python3 {{root}}/tools/generate_projection_fixture.py

# Fail if the projection fixture has drifted from PROJ. Needs pyproj.
projection-fixture-check:
    python3 {{root}}/tools/generate_projection_fixture.py --check

# Regenerate the GeoParquet reading fixtures. Needs geopandas and pyarrow.
geoparquet-fixture:
    python3 {{root}}/tools/generate_geoparquet_fixture.py

# Fail if the GeoParquet fixtures no longer match what geopandas writes.
#
# Not part of `just all`: unlike the projection fixture, this one is checked against a
# library whose output is expected to change between releases, and a drift here means
# "geopandas changed" rather than "this repo is wrong". Run it deliberately when
# upgrading geopandas.
geoparquet-fixture-check:
    python3 {{root}}/tools/generate_geoparquet_fixture.py --check

# Regenerate the Cloud-Optimized GeoTIFF fixtures. Needs the GDAL command-line tools.
cog-fixture:
    python3 {{root}}/tools/generate_cog_fixture.py

# Fail if the COG fixtures no longer match what GDAL's COG driver writes.
#
# Not part of `just all`, for the same reason as the GeoParquet check: drift here means
# GDAL changed, not that this repo is wrong. Run it deliberately when upgrading GDAL.
cog-fixture-check:
    python3 {{root}}/tools/generate_cog_fixture.py --check

# Re-vendor the published GeoParquet JSON Schemas. Needs network access.
geoparquet-schemas:
    python3 {{root}}/tools/vendor_geoparquet_schemas.py

# Fail if the vendored schemas have drifted from geoparquet.org. Needs network access.
#
# Not part of `just all`: a drift here means upstream published something, not that this
# repo is wrong. Run it deliberately.
geoparquet-schemas-check:
    python3 {{root}}/tools/vendor_geoparquet_schemas.py --check

# Validate a GeoParquet file's metadata, against the published schema and the file.
inspect url:
    cargo run -p iucn-rle-engine --features schema-validation --example inspect -- "{{url}}"

# Build the documentation site into docs/_build.
docs: docs-thresholds
    cd {{root}}/docs && npx -y mystmd@latest build --html

# Serve the documentation with live reload.
docs-serve: docs-thresholds
    cd {{root}}/docs && npx -y mystmd@latest start

# --- Everything -------------------------------------------------------------

# Lint, test, and prove every surface agrees. The full gate.
all: lint test deny wasm-check docs-thresholds-check conformance
    @echo "All surfaces built and in agreement."

# Print the version from every binding, to confirm they agree.
versions:
    @printf 'cli    '; cargo run -q -p iucn-rle-cli -- version
    @printf 'python '; {{root}}/target/venv/bin/python -c "import iucn_rle; print(iucn_rle.version())"
    @printf 'r      '; Rscript -e 'setwd("{{root}}/bindings/r/iucnrle"); pkgload::load_all(quiet=TRUE); cat(rle_version(), "\n")'
    @printf 'wasm   '; node -e "console.log(require('{{root}}/target/wasm-node/iucn_rle.js').version())"
