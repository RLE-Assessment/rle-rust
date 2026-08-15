// The JavaScript runner for the cross-language conformance corpus.
//
// Runs the identical JSON file as the Rust, Python, R and C ABI suites against the
// WASM build. Kept dependency-free on purpose: node --test is enough, and adding a
// test framework to verify a table of cases would be more machinery than the thing
// it verifies.
//
// Build first:
//   wasm-pack build bindings/wasm --target nodejs \
//       --out-dir ../../target/wasm-node --out-name iucn_rle

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { createRequire } from 'node:module';
import test from 'node:test';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

const wasm = require(resolve(here, '../target/wasm-node/iucn_rle.js'));
const corpus = JSON.parse(
  readFileSync(resolve(here, '../fixtures/cases/criterion_b.json'), 'utf8'),
);

test('corpus is not empty', () => {
  // Guards against a renamed fixture making the suite vacuously pass.
  assert.ok(corpus.cases.length >= 28, `only ${corpus.cases.length} cases loaded`);
});

for (const testCase of corpus.cases) {
  test(testCase.id, () => {
    const result = wasm.criterionB({
      eooKm2: testCase.eoo_km2 ?? undefined,
      eooLowerKm2: testCase.eoo_lower_km2 ?? undefined,
      eooUpperKm2: testCase.eoo_upper_km2 ?? undefined,
      aooCells: testCase.aoo_cells ?? undefined,
      clauses: testCase.clauses ?? [],
      locations: testCase.locations ?? undefined,
      noPlausibleThreats: testCase.no_plausible_threats ?? false,
      locationsInsufficientInformation:
        testCase.locations_insufficient_information ?? false,
    });

    const byCriterion = Object.fromEntries(
      result.criteria.map((c) => [c.criterion, c.category]),
    );

    for (const [key, expected] of Object.entries(testCase.expect)) {
      const actual = key === 'overall' ? result.overall : byCriterion[key.toUpperCase()];
      assert.equal(actual, expected, key);
    }

    const thresholds = Object.fromEntries(
      result.criteria.map((c) => [c.criterion, c.threshold_category]),
    );
    for (const [key, expected] of Object.entries(testCase.expect_threshold ?? {})) {
      assert.equal(thresholds[key.toUpperCase()], expected, `${key} threshold`);
    }
  });
}

test('thresholds shipped with the bundle match the engine', () => {
  assert.ok(wasm.thresholdsToml().includes('guidelines_version = "2.0"'));
  assert.ok(wasm.thresholdsToml().includes('criteria_version = "2.1"'));
  assert.equal(wasm.thresholdsSha256().length, 64);

  const result = wasm.criterionB({ eooKm2: 15000 });
  assert.equal(result.thresholds_sha256, wasm.thresholdsSha256());
});

test('clause (c) as a status is rejected', () => {
  // (c) is a count, not a status. Accepting a boolean here is the bug M1.5 fixed.
  assert.throws(
    () => wasm.criterionB({ eooKm2: 15000, clauses: [{ sub: 'c', status: 'met' }] }),
    /locations/,
  );
});

test('an unknown clause throws a useful error', () => {
  assert.throws(
    () => wasm.criterionB({ eooKm2: 15000, clauses: [{ sub: 'z', status: 'met' }] }),
    /a\|b/,
  );
});
