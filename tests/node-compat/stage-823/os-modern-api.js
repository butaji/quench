"use strict";

const assert = require("assert");
const osApi = require("node:os");

for (
  const name of [
    "availableParallelism",
    "getPriority",
    "setPriority",
    "machine",
    "version",
  ]
) {
  assert.strictEqual(typeof osApi[name], "function");
}
assert.strictEqual(typeof osApi.constants, "object");
assert.strictEqual(typeof osApi.availableParallelism(), "number");

console.log("os modern api passed");
