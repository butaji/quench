"use strict";

const assert = require("assert");
const moduleApi = require("module");

assert.strictEqual(moduleApi.isBuiltin("events"), true);
assert.strictEqual(moduleApi.isBuiltin("node:events"), true);
assert.strictEqual(moduleApi.isBuiltin("not-a-builtin"), false);
assert.strictEqual(moduleApi.builtinModules.includes("fs"), true);
assert.strictEqual(moduleApi.builtinModules.includes("events"), true);

console.log("module builtins passed");
