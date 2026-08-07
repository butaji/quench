"use strict";

const assert = require("assert");
const perfApi = require("node:perf_hooks");

assert.strictEqual(typeof perfApi.performance, "object");
for (
  const name of [
    "PerformanceObserver",
    "PerformanceEntry",
    "PerformanceMark",
    "PerformanceMeasure",
    "monitorEventLoopDelay",
    "createHistogram",
  ]
) {
  assert.strictEqual(typeof perfApi[name], "function");
}
assert.strictEqual(typeof perfApi.constants, "object");
assert.strictEqual(typeof perfApi.performance.now(), "number");

console.log("perf hooks api passed");
