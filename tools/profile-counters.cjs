#!/usr/bin/env node
"use strict";

// Run a workload once and emit stable JSON counters. perf is optional: on macOS
// /usr/bin/time supplies wall time and peak RSS while unavailable counters stay null.
const cp = require("child_process");
const fs = require("fs");
const DEFAULT_TIMEOUT_MS = 120_000;
// This tool collects evidence; it never authorizes speculative prefetch.
const [command, ...args] = process.argv.slice(2);
if (!command) {
  console.error("usage: profile-counters.cjs <command> [args...]");
  process.exit(2);
}
const start = process.hrtime.bigint();
const result = cp.spawnSync("/usr/bin/time", ["-l", command, ...args], {
  encoding: "utf8",
  timeout: DEFAULT_TIMEOUT_MS,
  killSignal: "SIGKILL",
});
const wallMs = Number(process.hrtime.bigint() - start) / 1e6;
if (result.error) throw result.error;
if (result.status !== 0) {
  process.stderr.write(result.stderr || "");
  process.exit(result.status || 1);
}
const metrics = {
  wall_ms: wallMs, peak_rss_bytes: null, cycles: null, instructions: null,
  branches: null, branch_misses: null, cache_misses: null, tlb_faults: null,
  allocations: null, copies: null,
};
for (const line of (result.stderr || "").split(/\n/)) {
  // macOS `/usr/bin/time -l` prints `maximum resident set size: N`, while
  // perf-style counters print `N cycles`. Accept both host spellings.
  const match = line.match(/^\s*(?:(\d+(?:\.\d+)?)\s+(.+?)|(.+?):\s*(\d+(?:\.\d+)?))\s*:?\s*$/i);
  if (!match) continue;
  const label = (match[2] || match[3] || "").trim().toLowerCase();
  const value = Number(match[1] || match[4]);
  const key = {
    "maximum resident set size": "peak_rss_bytes",
    cycles: "cycles", instructions: "instructions", branches: "branches",
    "branch-misses": "branch_misses", "cache-misses": "cache_misses",
    "dtlb-load-misses": "tlb_faults",
  }[label];
  if (key && Number.isFinite(value)) metrics[key] = value;
}
// Null is unavailable, never zero. Allocation/copy counters are unsupported.
const supported = Object.keys(metrics).filter((key) => metrics[key] !== null);
const unavailable = Object.keys(metrics).filter((key) => metrics[key] === null);
process.stdout.write(JSON.stringify({
  ...metrics,
  counter_contract: {
    version: 1,
    // A field appears in exactly one of these sets.  null is the only
    // unavailable representation; zero remains a valid observed count.
    supported,
    unavailable,
    limitations: [
      "null counters are unavailable, not zero",
      "allocation and copy counters are not measured",
      "manual prefetch requires independent before/after evidence",
    ],
  },
  prefetch_approval: {
    // Approval is an evidence gate, not a default optimization switch:
    // unavailable required hardware counters must keep it disabled.
    approved: metrics.cycles !== null && metrics.cache_misses !== null,
    reason: metrics.cycles === null || metrics.cache_misses === null
      ? "required hardware counters are unavailable"
      : "profiling does not prove manual prefetch is beneficial",
    required_counters: ["cache_misses", "cycles"],
    unavailable_required: ["cache_misses", "cycles"].filter((key) => metrics[key] === null),
  },
}) + "\n");
