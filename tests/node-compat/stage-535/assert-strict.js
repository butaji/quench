"use strict";

const assert = require("assert");
const strict = require("assert/strict");

assert.strictEqual(strict, assert);
strict.strictEqual(1, 1);
strict.notStrictEqual(1, "1");
assert.throws(() => strict.strictEqual(1, "1"));

console.log("assert strict passed");
