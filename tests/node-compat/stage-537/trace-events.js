"use strict";

const assert = require("assert");

for (const name of ["trace_events", "node:trace_events"]) {
  const traceEvents = require(name);
  assert.strictEqual(typeof traceEvents.createTracing, "function");
  assert.strictEqual(typeof traceEvents.getEnabledCategories, "function");
}

console.log("trace events api passed");
