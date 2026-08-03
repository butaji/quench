"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.report.getReport, "function");
const report = processApi.report.getReport();
assert.strictEqual(report.header.event, "JavaScript API");
assert.strictEqual(report.header.pid, processApi.pid);
assert.ok(Array.isArray(report.libuv));
assert.strictEqual(processApi.report.reportOnSignal, false);

console.log("process report passed");
