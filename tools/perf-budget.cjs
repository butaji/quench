#!/usr/bin/env node
"use strict";

// Deterministic, dependency-free budget gate. A benchmark command prints one
// JSON object; the gate compares supported numeric metrics against a checked-in
// budget. A metric value of null is an explicit "unsupported/unavailable"
// state and is not treated as zero or as a failure.
//
// Canonical sample shape: { metricName: number | null }, owned by the
// benchmark process for one run and finalized in its last JSON line. Missing
// keys and non-numeric, non-null values are invalid; null remains unsupported.
// The gate derives wall_ms at the host boundary when the sample omits it.
const fs = require("fs");
const cp = require("child_process");
const DEFAULT_TIMEOUT_MS = 120_000;

const [budgetFile, command, ...args] = process.argv.slice(2);
if (!budgetFile || !command) {
  console.error("usage: perf-budget.cjs <budget.json> <command> [args...]");
  process.exit(2);
}
let budget;
try {
  budget = JSON.parse(fs.readFileSync(budgetFile, "utf8"));
} catch (error) {
  console.error(`invalid performance budget: ${error.message}`);
  process.exit(1);
}
if (!budget || typeof budget !== "object" || Array.isArray(budget)) {
  console.error("performance budget must be a JSON object");
  process.exit(1);
}
const budgetEntries = Object.entries(budget);
if (!budgetEntries.length) {
  console.error("performance budget must define at least one metric");
  process.exit(1);
}
for (const [name, limit] of budgetEntries) {
  if (!name || typeof limit !== "number" || !Number.isFinite(limit) || limit < 0) {
    console.error(`invalid performance budget limit for ${name || "<unnamed>"}`);
    process.exit(1);
  }
}
const started = process.hrtime.bigint();
const result = cp.spawnSync(command, args, {
  encoding: "utf8",
  timeout: DEFAULT_TIMEOUT_MS,
  killSignal: "SIGKILL",
});
const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
if (result.error) throw result.error;
if (result.status !== 0) {
  process.stderr.write(result.stderr || "");
  process.exit(result.status || 1);
}
const lines = result.stdout.trim().split(/\n/).filter(Boolean);
if (!lines.length) {
  console.error("performance budget command produced no JSON sample");
  process.exit(1);
}
let sample;
try {
  sample = JSON.parse(lines.at(-1));
} catch (error) {
  console.error(`invalid performance sample: ${error.message}`);
  process.exit(1);
}
if (!sample || typeof sample !== "object" || Array.isArray(sample)) {
  console.error("performance sample must be a JSON object");
  process.exit(1);
}
// An omitted host-derived metric is completed at the boundary; an explicit
// null remains unsupported and must not be changed into a measured value.
const metrics = Object.hasOwn(sample, "wall_ms")
  ? { ...sample }
  : { ...sample, wall_ms: elapsedMs };
const failures = [];
for (const [name, limit] of Object.entries(budget)) {
  const present = Object.hasOwn(metrics, name);
  const value = metrics[name];
  // The wire invariant is number | null: never coerce strings, booleans, or
  // non-finite JSON numbers into a budget value. Missing metrics are invalid
  // unless they are the host-derived wall_ms completed above.
  if (value === null) continue;
  if (!present || typeof value !== "number" || !Number.isFinite(value)) {
    const rendered = present ? String(value) : "missing";
    failures.push(`${name}: ${rendered} > ${limit}`);
  } else if (value > limit) {
    failures.push(`${name}: ${value} > ${limit}`);
  }
}
console.log(JSON.stringify({ metrics, budget, ok: failures.length === 0, failures }));
if (failures.length) process.exit(1);
