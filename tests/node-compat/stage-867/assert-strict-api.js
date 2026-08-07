"use strict";

const assert = require("node:assert");
const strict = require("node:assert/strict");

for (
  const name of [
    "strictEqual",
    "notStrictEqual",
    "deepStrictEqual",
    "notDeepStrictEqual",
    "throws",
    "rejects",
    "doesNotThrow",
  ]
) {
  assert.strictEqual(typeof strict[name], "function");
}
assert.strictEqual(strict.strict, strict);
assert.strictEqual(typeof strict.AssertionError, "function");
strict.strictEqual(1, 1);

console.log("assert strict api passed");
