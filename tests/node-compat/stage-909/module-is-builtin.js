"use strict";

const assert = require("assert");
const { isBuiltin } = require("module");

assert.strictEqual(isBuiltin("http"), true);
assert.strictEqual(isBuiltin("sys"), true);
assert.strictEqual(isBuiltin("node:test"), true);
assert.strictEqual(isBuiltin("internal/errors"), false);
assert.strictEqual(isBuiltin("test"), false);
assert.strictEqual(isBuiltin(""), false);
assert.strictEqual(isBuiltin(undefined), false);

console.log("module is builtin passed");
