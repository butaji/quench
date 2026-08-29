#!/usr/bin/env node
"use strict";

const cp = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "..", "..");
const lanes = path.join(root, "tests/lanes");
const binary = path.resolve(root, process.env.QUENCH_TRACE_BINARY ||
  "target-exec-trace/bench-throughput/quench-node");
function readPath(value, dotted) {
  return dotted.split(".").reduce((current, key) => current?.[key], value);
}

function resolve(snapshot, source) {
  if (typeof source === "string") return readPath(snapshot, source);
  if (source.sum) return source.sum.reduce((sum, item) => sum + resolve(snapshot, item), 0);
  const rows = readPath(snapshot, source.top.path) || [];
  const row = rows.find((entry) => entry[source.top.key] === source.top.value);
  return row?.[source.top.field || "count"] || 0;
}

function measure(snapshot, fact) {
  const numerator = resolve(snapshot, fact.numerator);
  const denominator = fact.denominator === undefined ? 1 : resolve(snapshot, fact.denominator);
  return { numerator, denominator, value: denominator ? numerator / denominator : null };
}

function failuresFor(snapshot, want) {
  return Object.entries(want.metrics).flatMap(([name, fact]) => {
    const measured = measure(snapshot, fact);
    if (measured.denominator < (fact.min_traffic || 0)) {
      return [{ name, ...measured, reason: `traffic below ${fact.min_traffic}` }];
    }
    if (!Number.isFinite(measured.value)) {
      return [{ name, ...measured, reason: "measurement unavailable" }];
    }
    if (fact.min !== undefined && measured.value < fact.min) {
      return [{ name, ...measured, reason: `below ${fact.min}` }];
    }
    if (fact.max !== undefined && measured.value > fact.max) {
      return [{ name, ...measured, reason: `above ${fact.max}` }];
    }
    return [];
  });
}

function run(id) {
  const source = path.join(lanes, `${id}.js`);
  const want = JSON.parse(fs.readFileSync(path.join(lanes, `${id}.want.json`), "utf8"));
  const result = cp.spawnSync(binary, [source], {
    encoding: "utf8",
    env: { ...process.env, QUENCH_EXEC_TRACE: "1" },
    maxBuffer: 64 * 1024 * 1024,
    timeout: 120_000,
  });
  const line = result.stderr?.split(/\n/).find((entry) => entry.startsWith("QUENCH_EXEC_TRACE "));
  if (!line) throw new Error(`${id}: trace missing (status ${result.status})\n${result.stderr || ""}`);
  const snapshot = JSON.parse(line.slice("QUENCH_EXEC_TRACE ".length));
  const failures = failuresFor(snapshot, want);
  const measurements = Object.fromEntries(Object.entries(want.metrics)
    .map(([name, fact]) => [name, measure(snapshot, fact)]));
  process.stdout.write(`${JSON.stringify({ id, passed: !failures.length, measurements })}\n`);
  for (const failure of failures) {
    console.error(`${id}.${failure.name}: ${failure.value ?? "missing"}; ${failure.reason} ` +
      `(numerator=${failure.numerator}, denominator=${failure.denominator})`);
  }
  return failures.length === 0 && result.status === 0;
}

function main() {
  const requested = process.argv.slice(2);
  const ids = requested.length ? requested : fs.readdirSync(lanes)
    .filter((name) => name.endsWith(".want.json"))
    .map((name) => name.slice(0, -".want.json".length)).sort();
  let passed = true;
  for (const id of ids) passed = run(id) && passed;
  process.exitCode = passed ? 0 : 1;
}

module.exports = { failuresFor, measure, readPath, resolve };
if (require.main === module) main();
