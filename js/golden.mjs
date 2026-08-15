// The JavaScript runner for rle-python's golden null-island dataset.
//
// Runs the identical fixture as the Rust, Python and R suites.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { createRequire } from 'node:module';
import test from 'node:test';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

const wasm = require(resolve(here, '../target/wasm-node/iucn_rle.js'));
const fixture = JSON.parse(
  readFileSync(resolve(here, '../fixtures/cases/null_island_aoo.json'), 'utf8'),
);

function metrics() {
  const polygons = fixture.features.map((f) => ({
    ecosystem: f.code,
    rings: f.rings,
  }));
  const result = wasm.distributionMetrics(polygons);
  return Object.fromEntries(result.ecosystems.map((e) => [e.ecosystem, e]));
}

test('fixture is present', () => {
  assert.equal(fixture.features.length, 3);
});

test('occupied cell counts match rle-python', () => {
  const byEcosystem = metrics();
  for (const feature of fixture.features) {
    const expected = fixture.expected_cells.filter(
      (c) => (c.fractions[feature.code] ?? 0) > 0,
    ).length;
    assert.equal(byEcosystem[feature.code].occupied_cell_count, expected, feature.code);
  }
});

test('published metrics match', () => {
  // From the committed workshop notebook: "EOO is 73.2 km2", "AOO is 4 cells".
  const published = fixture.published_metrics;
  const byEcosystem = metrics();

  for (const [code, expected] of Object.entries(published.ecosystems)) {
    const actual = byEcosystem[code];
    assert.ok(
      Math.abs(actual.eoo_km2 - expected.eoo_km2) < published.eoo_tolerance_km2,
      `${code} EOO: expected ${expected.eoo_km2}, got ${actual.eoo_km2}`,
    );
    assert.equal(actual.aoo_cells, expected.aoo_cells, `${code} AOO`);
  }
});

test('out-of-range coordinates are rejected', () => {
  assert.throws(
    () =>
      wasm.distributionMetrics([
        { ecosystem: 'forest', rings: [[[10, 200], [11, 201]]] },
      ]),
    /latitude/,
  );
});
