"use strict";

const assert = require("assert");
const nodeTest = require("node:test");

assert.strictEqual(typeof assert.partialDeepStrictEqual, "function");
assert.partialDeepStrictEqual(new Uint8Array([1, 2]), new Uint8Array([1, 2]));
assert.strictEqual(typeof nodeTest.test, "function");
assert.strictEqual(typeof nodeTest.suite, "function");
