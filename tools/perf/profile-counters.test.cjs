"use strict";
const assert = require("assert");
const cp = require("child_process");
const path = require("path");

const output = cp.execFileSync(process.execPath, [path.join(__dirname, "profile-counters.cjs"), "/usr/bin/true"], { encoding: "utf8" });
const report = JSON.parse(output);
const fields = ["cycles", "instructions", "branches", "branch_misses", "cache_misses", "tlb_faults", "allocations", "copies"];
const reportKeys = [
  "wall_ms", "peak_rss_bytes", ...fields, "counter_contract", "prefetch_approval",
];
assert.deepStrictEqual(Object.keys(report).sort(), reportKeys.sort());
assert.strictEqual(report.counter_contract.version, 1);
assert.deepStrictEqual(
  new Set(report.counter_contract.supported.concat(report.counter_contract.unavailable)).size,
  fields.length + 2,
);
assert.strictEqual(
  report.counter_contract.supported.length + report.counter_contract.unavailable.length,
  fields.length + 2,
);
for (const field of fields) {
  assert.ok(report.counter_contract.supported.includes(field) || report.counter_contract.unavailable.includes(field));
  if (report.counter_contract.unavailable.includes(field)) assert.strictEqual(report[field], null);
  else assert.ok(Number.isFinite(report[field]) && report[field] >= 0);
}
assert.ok(Number.isFinite(report.wall_ms) && report.wall_ms >= 0);
assert.ok(report.peak_rss_bytes === null || (Number.isFinite(report.peak_rss_bytes) && report.peak_rss_bytes >= 0));
assert.deepStrictEqual(report.prefetch_approval.required_counters, ["cache_misses", "cycles"]);
assert.strictEqual(report.prefetch_approval.approved, false);
assert.ok(report.prefetch_approval.unavailable_required.length > 0);
assert.ok(report.prefetch_approval.unavailable_required.includes("cache_misses"));
assert.strictEqual(report.prefetch_approval.reason, "required hardware counters are unavailable");
console.log("profile counter contract: ok");
