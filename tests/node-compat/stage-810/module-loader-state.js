"use strict";

const assert = require("assert");
const Module = require("module").Module;

assert.strictEqual(typeof Module._extensions, "object");
assert.strictEqual(Array.isArray(Module.globalPaths), true);
assert.strictEqual(typeof Module._pathCache, "object");
assert.strictEqual(typeof Module._cache, "object");

console.log("module loader state passed");
