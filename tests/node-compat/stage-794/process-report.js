"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.report, "object");
for (const name of ["getReport", "writeReport"]) {
  assert.strictEqual(typeof processApi.report[name], "function");
}
assert.strictEqual(typeof processApi.report.compact, "boolean");
assert.strictEqual(typeof processApi.report.directory, "string");
assert.strictEqual(typeof processApi.report.signal, "string");

console.log("process report passed");
