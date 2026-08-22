#!/usr/bin/env node
"use strict";

// Deterministic, dependency-free budget gate. A benchmark command prints one
// JSON object; the gate compares numeric metrics against a checked-in budget.
const fs = require("fs");
const cp = require("child_process");

const [budgetFile, command, ...args] = process.argv.slice(2);
if (!budgetFile || !command) {
  console.error("usage: perf-budget.cjs <budget.json> <command> [args...]");
  process.exit(2);
}
const budget = JSON.parse(fs.readFileSync(budgetFile, "utf8"));
const started = process.hrtime.bigint();
const result = cp.spawnSync(command, args, { encoding: "utf8" });
const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
if (result.error) throw result.error;
if (result.status !== 0) {
  process.stderr.write(result.stderr || "");
  process.exit(result.status || 1);
}
const lines = result.stdout.trim().split(/\n/).filter(Boolean);
const sample = JSON.parse(lines.at(-1));
const metrics = { ...sample, wall_ms: sample.wall_ms ?? elapsedMs };
const failures = [];
for (const [name, limit] of Object.entries(budget)) {
  if (typeof metrics[name] !== "number" || metrics[name] > limit) {
    failures.push(`${name}: ${metrics[name] ?? "missing"} > ${limit}`);
  }
}
console.log(JSON.stringify({ metrics, budget, ok: failures.length === 0, failures }));
if (failures.length) process.exit(1);
