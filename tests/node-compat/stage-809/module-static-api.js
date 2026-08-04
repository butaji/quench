"use strict";

const assert = require("assert");
const moduleApi = require("module");
const Module = moduleApi.Module;

assert.strictEqual(typeof Module, "function");
assert.strictEqual(typeof Module.isBuiltin, "function");
assert.strictEqual(typeof Module.createRequire, "function");
assert.strictEqual(Array.isArray(Module.builtinModules), true);
assert.strictEqual(typeof Module._cache, "object");
assert.strictEqual(Module.isBuiltin("fs"), true);

console.log("module static api passed");
