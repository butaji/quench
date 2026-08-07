"use strict";

const assert = require("assert");
const util = require("node:util");

for (
  const name of [
    "parseArgs",
    "styleText",
    "transferableAbortController",
    "transferableAbortSignal",
  ]
) {
  assert.strictEqual(typeof util[name], "function");
}
assert.strictEqual(typeof util.parseArgs({ args: [], options: {} }), "object");
assert.strictEqual(typeof util.styleText("red", "value"), "string");

console.log("util modern api passed");
