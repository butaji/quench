"use strict";

const assert = require("assert");
const Module = require("module").Module;

assert.strictEqual(typeof Module._resolveFilename, "function");
assert.strictEqual(typeof Module._resolveLookupPaths, "function");
assert.strictEqual(typeof Module._load, "function");
assert.strictEqual(Module._resolveFilename("node:assert"), "node:assert");

console.log("module resolution helpers passed");
