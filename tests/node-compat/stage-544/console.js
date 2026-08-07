"use strict";

const assert = require("assert");
const consoleApi = require("console");

assert.strictEqual(typeof consoleApi.Console, "function");
for (
  const method of [
    "table",
    "trace",
    "assert",
    "group",
    "groupCollapsed",
    "groupEnd",
  ]
) {
  assert.strictEqual(typeof consoleApi.Console.prototype[method], "function");
}
assert.strictEqual(typeof consoleApi.log, "function");

console.log("console surface passed");
