#!/usr/bin/env node
"use strict";

// Run a workload once and emit stable JSON counters. perf is optional: on macOS
// /usr/bin/time supplies wall time and peak RSS while unavailable counters stay null.
const cp = require("child_process");
const fs = require("fs");
const [command, ...args] = process.argv.slice(2);
if (!command) {
  console.error("usage: profile-counters.cjs <command> [args...]");
  process.exit(2);
}
const start = process.hrtime.bigint();
const result = cp.spawnSync("/usr/bin/time", ["-l", command, ...args], { encoding: "utf8" });
const wallMs = Number(process.hrtime.bigint() - start) / 1e6;
if (result.error) throw result.error;
if (result.status !== 0) {
  process.stderr.write(result.stderr || "");
  process.exit(result.status || 1);
}
const metrics = { wall_ms: wallMs, peak_rss_bytes: null, cycles: null, instructions: null,
  branches: null, branch_misses: null, cache_misses: null, tlb_faults: null,
  allocations: null, copies: null };
for (const line of (result.stderr || "").split(/\n/)) {
  const match = line.match(/^\s*([\d.]+)\s+(maximum resident set size|cycles|instructions|branches|branch-misses|cache-misses|dTLB-load-misses):?/i);
  if (!match) continue;
  const value = Number(match[1]);
  const key = { "maximum resident set size": "peak_rss_bytes", cycles: "cycles", instructions: "instructions", branches: "branches", "branch-misses": "branch_misses", "cache-misses": "cache_misses", "dtlb-load-misses": "tlb_faults" }[match[2].toLowerCase()];
  if (key) metrics[key] = value;
}
process.stdout.write(JSON.stringify(metrics) + "\n");
