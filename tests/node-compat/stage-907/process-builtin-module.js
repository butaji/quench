"use strict";

const assert = require("assert");

assert.strictEqual(typeof process.getBuiltinModule, "function");
const crypto = process.getBuiltinModule("crypto");
assert.strictEqual(typeof crypto.createHash, "function");
assert.strictEqual(
  process.getBuiltinModule("definitely-not-a-module"),
  undefined,
);

console.log("process builtin module passed");
