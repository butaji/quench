"use strict";

const assert = require("assert");
const sys = require("node:sys");

for (
  const name of [
    "format",
    "debug",
    "inspect",
    "log",
    "inherits",
    "isArray",
    "isBoolean",
    "isNull",
  ]
) {
  assert.strictEqual(typeof sys[name], "function");
}
assert.strictEqual(sys.isArray([]), true);
assert.strictEqual(sys.isBoolean(false), true);
assert.strictEqual(sys.isNull(null), true);

console.log("sys api passed");
