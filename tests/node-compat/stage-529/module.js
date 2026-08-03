"use strict";

const assert = require("assert");
const moduleApi = require("module");

assert.ok(moduleApi.builtinModules.includes("fs"));
assert.strictEqual(moduleApi.isBuiltin("node:fs"), true);
assert.strictEqual(moduleApi.isBuiltin("not-a-builtin"), false);
assert.strictEqual(typeof moduleApi.createRequire(__filename), "function");
assert.ok(moduleApi._cache);
assert.ok(moduleApi._extensions);

console.log("module surface passed");
