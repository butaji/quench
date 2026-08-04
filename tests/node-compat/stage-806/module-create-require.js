"use strict";

const assert = require("assert");
const moduleApi = require("module");

assert.strictEqual(typeof moduleApi.createRequire, "function");
const localRequire = moduleApi.createRequire(__filename);
assert.strictEqual(typeof localRequire, "function");
assert.strictEqual(typeof localRequire("node:assert").strictEqual, "function");
assert.strictEqual(moduleApi.isBuiltin("node:fs"), true);

console.log("module create require passed");
