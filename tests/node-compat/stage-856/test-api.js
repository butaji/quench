"use strict";

const assert = require("assert");
const test = require("node:test");

for (
  const name of [
    "test",
    "describe",
    "it",
    "before",
    "after",
    "beforeEach",
    "afterEach",
  ]
) {
  assert.strictEqual(typeof test[name], "function");
}
assert.strictEqual(typeof test.run, "function");
assert.strictEqual(typeof test.mock, "object");
assert.strictEqual(typeof test.snapshot, "function");

console.log("test api passed");
