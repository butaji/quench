#!/usr/bin/env node
"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs");
const { execFileSync } = require("node:child_process");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const scripts = [
  ["value", "value-representation-benchmark.cjs", "QUENCH_VALUE_ITERATIONS", 257],
  ["dispatch", "dispatch-benchmark.cjs", "QUENCH_DISPATCH_ITERATIONS", 257],
  ["layout", "instruction-layout-benchmark.cjs", "QUENCH_LAYOUT_ITERATIONS", 257],
];
function run(runtime, script, variable, iterations) {
  const source = fs.readFileSync(path.join(__dirname, script), "utf8").replace(/^#!.*\n/, "");
  const args = runtime === process.execPath ? [path.join(__dirname, script)] : ["-e", source];
  const output = execFileSync(runtime, args, {
    cwd: root,
    env: { ...process.env, [variable]: String(iterations) },
    encoding: "utf8",
  });
  return JSON.parse(output);
}
function normalized(value) {
  return JSON.stringify(value, (key, item) => key === "wall_ms" ? undefined : item);
}
for (const [name, script, variable, iterations] of scripts) {
  const nodeResult = run(process.execPath, script, variable, iterations);
  const quench = process.env.QUENCH_NODE || path.join(root, "target-native/release-thin/quench-node");
  const quenchResult = run(quench, script, variable, iterations);
  assert.equal(normalized(quenchResult), normalized(nodeResult), `${name} output contract`);
  assert.ok(nodeResult.results.length >= 2, `${name} has comparison results`);
  for (const result of nodeResult.results) {
    assert.equal(result.iterations, iterations);
    assert.equal(typeof result.checksum, "number");
    assert.equal(typeof result.wall_ms, "number");
    assert.ok(result.wall_ms >= 0);
  }
}

// This is a contract probe, not a throughput sample: invalid inputs must fail
// before either runtime allocates its measurement arrays.
for (const [name, script, variable] of scripts) {
  for (const invalid of ["0", "-1", "1.5", "not-a-number"]) {
    assert.throws(
      () => run(process.execPath, script, variable, invalid),
      /must be a positive safe integer/,
      `${name} rejects ${invalid}`,
    );
  }
}
console.log(`microbenchmark contracts passed (${scripts.length} benchmarks, Node/quench-node)`);
