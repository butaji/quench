"use strict";

const assert = require("assert");
const workers = require("node:worker_threads");

for (
  const name of [
    "setEnvironmentData",
    "getEnvironmentData",
    "markAsUntransferable",
    "markAsUncloneable",
    "isMarkedAsUncloneable",
    "moveMessagePortToContext",
  ]
) {
  assert.strictEqual(typeof workers[name], "function");
}
assert.strictEqual(typeof workers.getEnvironmentData("missing"), "undefined");

console.log("worker helpers api passed");
