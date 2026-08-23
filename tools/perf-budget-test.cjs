#!/usr/bin/env node
"use strict";
const assert = require("assert");
const cp = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

const gate = path.join(__dirname, "perf-budget.cjs");
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "quench-perf-budget-"));
const budgetFile = path.join(dir, "budget.json");
fs.writeFileSync(budgetFile, JSON.stringify({ cycles: 10, wall_ms: 10 }));
const run = (sample) => cp.spawnSync(process.execPath, [gate, budgetFile, process.execPath, "-e", `console.log(${JSON.stringify(JSON.stringify(sample))})`], { encoding: "utf8" });


let result = run({ cycles: null, wall_ms: null });
assert.strictEqual(result.status, 0, result.stdout + result.stderr);
assert.deepStrictEqual(JSON.parse(result.stdout).failures, []);

result = run({ wall_ms: 1 });
assert.strictEqual(result.status, 1);
assert.match(result.stdout, /cycles: missing/);

result = run({ cycles: "NaN", wall_ms: 1 });
assert.strictEqual(result.status, 1);
assert.match(result.stdout, /cycles: NaN/);

result = run({ cycles: 11, wall_ms: 1 });
assert.strictEqual(result.status, 1);
assert.match(result.stdout, /cycles: 11/);

const runRaw = (output) => cp.spawnSync(process.execPath, [gate, budgetFile, process.execPath, "-e", `process.stdout.write(${JSON.stringify(output)})`], { encoding: "utf8" });

result = runRaw("{");
assert.strictEqual(result.status, 1);
assert.match(result.stderr, /invalid performance sample/);

for (const malformed of ["null", "[]", "42"]) {
  result = runRaw(malformed);
  assert.strictEqual(result.status, 1);
  assert.match(result.stderr, /performance sample must be a JSON object/);
}

for (const [output, label] of [
  ['{"cycles":1e999,"wall_ms":1}', "Infinity"],
  ['{"cycles":"NaN","wall_ms":1}', "NaN"],
  ['{"cycles":"oops","wall_ms":1}', "oops"],
]) {
  result = runRaw(output);
  assert.strictEqual(result.status, 1);
  assert.match(result.stdout, new RegExp(`cycles: ${label}`));
}

result = run({ cycles: null, wall_ms: null });
assert.strictEqual(result.status, 0);
assert.deepStrictEqual(JSON.parse(result.stdout).failures, []);

result = run({ cycles: 11, wall_ms: 11 });
assert.strictEqual(result.status, 1);
assert.deepStrictEqual(JSON.parse(result.stdout).failures, ["cycles: 11 > 10", "wall_ms: 11 > 10"]);

const invalidBudget = path.join(dir, "invalid-budget.json");
fs.writeFileSync(invalidBudget, JSON.stringify({ cycles: "10" }));
result = cp.spawnSync(process.execPath, [gate, invalidBudget, process.execPath, "-e", "console.log('{}')"], { encoding: "utf8" });
assert.strictEqual(result.status, 1);
assert.match(result.stderr, /invalid performance budget limit/);

const emptyBudget = path.join(dir, "empty-budget.json");
fs.writeFileSync(emptyBudget, "{}");
result = cp.spawnSync(process.execPath, [gate, emptyBudget, process.execPath, "-e", "console.log('{}')"], { encoding: "utf8" });
assert.strictEqual(result.status, 1);
assert.match(result.stderr, /must define at least one metric/);

const negativeBudget = path.join(dir, "negative-budget.json");
fs.writeFileSync(negativeBudget, JSON.stringify({ cycles: -1 }));
result = cp.spawnSync(process.execPath, [gate, negativeBudget, process.execPath, "-e", "console.log('{}')"], { encoding: "utf8" });
assert.strictEqual(result.status, 1);
assert.match(result.stderr, /invalid performance budget limit/);

console.log("perf-budget: source shape, null, missing, nonfinite, and over-budget metrics verified");

