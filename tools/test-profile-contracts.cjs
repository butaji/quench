#!/usr/bin/env node
"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { formatViolations, violations } = require("./lib/profile-contracts.cjs");

const contracts = {
  defaults: { "ratios.decode": { max: 0.02 } },
  benchmarks: { deltablue: { score: { min: 5000 }, "lanes.l3.handlers": { max: 0 } } },
};
const passing = { score: 5000, ratios: { decode: 0.01 }, lanes: { l3: { handlers: 0 } } };
assert.deepEqual(violations(passing, contracts, "deltablue"), []);

const failing = { score: 85, ratios: { decode: 0.2 }, lanes: { l3: { handlers: 3 } } };
const failures = violations(failing, contracts, "deltablue");
assert.deepEqual(failures.map(({ path }) => path), ["ratios.decode", "score", "lanes.l3.handlers"]);
assert.match(formatViolations("deltablue", failures), /score = 85; below 5000/);

const missing = violations({ score: 5000, ratios: {}, lanes: { l3: { handlers: 0 } } }, contracts, "deltablue");
assert.equal(missing[0].reason, "missing numeric measurement");

const declared = JSON.parse(fs.readFileSync(
  path.join(__dirname, "../quench-bench/profile-contracts.json"), "utf8"));
assert.deepEqual(Object.keys(declared.benchmarks).sort(), [
  "crypto", "deltablue", "earley-boyer", "navier-stokes",
  "raytrace", "regexp", "richards", "splay",
]);
for (const rules of [declared.defaults, ...Object.values(declared.benchmarks)]) {
  for (const [metric, rule] of Object.entries(rules)) {
    assert.match(metric, /^(host|lanes|ratios|score|vm)(\.|$)/);
    assert.ok(rule.min !== undefined || rule.max !== undefined, `${metric} has no bound`);
    if (rule.min !== undefined && rule.max !== undefined) assert.ok(rule.min <= rule.max);
  }
}
console.log("execution profile contract tests: ok");
