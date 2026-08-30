"use strict";

// V8-v7-style micro runner: compile one source body, warm it to a steady
// tier, then measure repeated windows and report time per workload call. The
// window count is shared across runtimes, while the number of calls in each
// window is allowed to vary: this is the same operation-normalisation used by
// the v8-v7 harness and avoids making a fixed call count either too noisy or
// needlessly expensive for a particular fixture.
const fs = require("node:fs");
const path = require("node:path");

const sourcePath = process.env.QUENCH_MICRO_PATH || process.argv[2];
if (!sourcePath) throw new Error("usage: harness.cjs MICRO.js");
const source = fs.readFileSync(sourcePath, "utf8");
const manifest = JSON.parse(fs.readFileSync(path.join(path.dirname(sourcePath), "manifest.json"), "utf8"));
const caseFile = path.basename(sourcePath);
const metadata = manifest.cases.find((item) => item.file === caseFile);
if (!metadata) throw new Error(`case is missing from manifest: ${caseFile}`);
const windowMs = Number(process.env.QUENCH_MICRO_WINDOW_MS || 250);
const measuredWindows = Number(process.env.QUENCH_MICRO_WINDOWS || 5);
const minimumRuns = Number(process.env.QUENCH_MICRO_MIN_RUNS || 32);
const clockBatch = Number(process.env.QUENCH_MICRO_CLOCK_BATCH || Math.min(64, minimumRuns));
for (const [name, value] of [["QUENCH_MICRO_WINDOW_MS", windowMs], ["QUENCH_MICRO_WINDOWS", measuredWindows], ["QUENCH_MICRO_MIN_RUNS", minimumRuns], ["QUENCH_MICRO_CLOCK_BATCH", clockBatch]]) {
  if (!Number.isSafeInteger(value) || value <= 0) throw new Error(`${name} must be a positive integer`);
}
// Return the fixture's declared result so legacy micros without microRun are
// still checked. This is protocol plumbing, not fixture inspection.
const body = new Function(`${source}\n;return (typeof result === "undefined" ? undefined : result);`);
const originalLog = console.log;
console.log = () => {};
body();
const workload = typeof globalThis.microRun === "function" ? globalThis.microRun : body;
// Let each runtime reach its ordinary optimized/steady execution tier before
// timing. Warmup is deliberately not reported as work.
let warmup = 0;
const warmupStarted = process.hrtime.bigint();
while (Number(process.hrtime.bigint() - warmupStarted) / 1e6 < windowMs) {
  for (let i = 0; i < clockBatch; i++) {
    workload();
    warmup++;
  }
}
let lastValue;
let iterations = 0;
let elapsedNs = 0;
for (let window = 0; window < measuredWindows; window++) {
  const started = process.hrtime.bigint();
  let calls = 0;
  do {
    for (let i = 0; i < clockBatch; i++) {
      lastValue = workload();
      calls++;
    }
  } while (calls < minimumRuns || Number(process.hrtime.bigint() - started) / 1e6 < windowMs);
  elapsedNs += Number(process.hrtime.bigint() - started);
  iterations += calls;
}
console.log = originalLog;
const serialized = lastValue === undefined ? "undefined" : JSON.stringify(lastValue);
let checksum = 2_166_136_261;
for (let i = 0; i < serialized.length; i++) {
  checksum ^= serialized.charCodeAt(i);
  checksum = Math.imul(checksum, 16_777_619) >>> 0;
}
process.stdout.write(JSON.stringify({
  id: metadata.id,
  family: metadata.family,
  operation: metadata.operation,
  memory_profile: metadata.memory_profile,
  work_units: metadata.work_units,
  iterations,
  warmup,
  elapsed_ns: elapsedNs,
  elapsed_ns_per_work_unit: elapsedNs / (iterations * metadata.work_units),
  checksum,
}));
