"use strict";

const assert = require("assert");
const traceEvents = require("node:trace_events");

assert.strictEqual(typeof traceEvents.createTracing, "function");
assert.strictEqual(typeof traceEvents.getEnabledCategories, "function");
const tracing = traceEvents.createTracing({
  categories: ["node"],
  enabled: false,
});
assert.strictEqual(typeof tracing.enable, "function");
assert.strictEqual(typeof tracing.disable, "function");
assert.strictEqual(typeof tracing.enabled, "boolean");

console.log("trace events api passed");
