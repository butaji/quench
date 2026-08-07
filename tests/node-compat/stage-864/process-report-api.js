"use strict";

const assert = require("assert");
const report = require("node:process").report;

for (
  const name of [
    "writeReport",
    "getReport",
    "directory",
    "filename",
    "compact",
    "signal",
    "reportOnFatalError",
    "reportOnSignal",
  ]
) {
  assert.ok(name in report);
}
assert.strictEqual(typeof report.writeReport, "function");
assert.strictEqual(typeof report.getReport, "function");
assert.strictEqual(typeof report.directory, "string");
assert.strictEqual(typeof report.compact, "boolean");

console.log("process report api passed");
