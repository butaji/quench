"use strict";

const assert = require("node:assert");

assert.strictEqual(typeof assert.strictEqual, "function");
assert.strictEqual(typeof assert.deepStrictEqual, "function");
assert.strictEqual(typeof assert.throws, "function");
assert.strictEqual(typeof assert.rejects, "function");
assert.strictEqual(typeof assert.AssertionError, "function");
assert.strictEqual(assert.strictEqual(1, 1), undefined);

console.log("assert core api passed");
