#!/usr/bin/env node
"use strict";
const assert = require("assert");
const path = require("path");
const bounded = require("./lib/bounded-process.cjs");

const workload = path.join(__dirname, "perf-workload.cjs");
const result = bounded.spawnSync(process.execPath, [workload], {
  env: { ...process.env, QUENCH_PERF_ITERATIONS: "32" },
  encoding: "utf8"
});
assert.strictEqual(result.status, 0, result.stderr);
const lines = result.stdout.trim().split(/\n/).filter(Boolean);
assert.strictEqual(lines.length, 1, "workload owns exactly one final snapshot");
const snapshot = JSON.parse(lines[0]);
const expected = [
  "iterations",
  "checksum",
  "allocations",
  "copies",
  "bytes",
  "peak_rss",
  "wall_ms"
];
assert.deepStrictEqual(Object.keys(snapshot).sort(), expected.sort());
for (const key of [
  "iterations",
  "checksum",
  "allocations",
  "copies",
  "bytes",
  "peak_rss"
]) {
  assert.ok(
    Number.isInteger(snapshot[key]) && snapshot[key] >= 0,
    `${key} must be a non-negative integer`
  );
}
assert.ok(snapshot.peak_rss > 0);
assert.ok(Number.isFinite(snapshot.wall_ms) && snapshot.wall_ms >= 0);
assert.strictEqual(snapshot.iterations, 32);
assert.strictEqual(snapshot.allocations, 32);
assert.strictEqual(snapshot.copies > 0, true);
assert.strictEqual(snapshot.bytes > snapshot.copies, true);
assert.strictEqual(snapshot.checksum > 0, true);

// Schema boundary: absent and null values are invalid for source-owned fields;
// consumers must not silently convert either state into a measurement.
for (const key of expected) {
  const missing = { ...snapshot };
  delete missing[key];
  assert.strictEqual(Object.hasOwn(missing, key), false);
  const nullable = { ...snapshot, [key]: null };
  assert.strictEqual(nullable[key], null);
}
console.log("perf-workload: JSON snapshot schema and invariants verified");
