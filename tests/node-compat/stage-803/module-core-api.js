"use strict";

const assert = require("assert");
const moduleApi = require("module");

assert.strictEqual(Array.isArray(moduleApi.builtinModules), true);
assert.strictEqual(moduleApi.builtinModules.length > 0, true);
for (
  const name of [
    "isBuiltin",
    "createRequire",
    "findSourceMap",
    "syncBuiltinESMExports",
  ]
) {
  assert.strictEqual(typeof moduleApi[name], "function");
}
assert.strictEqual(moduleApi.isBuiltin("fs"), true);
assert.strictEqual(moduleApi.isBuiltin("not-a-builtin"), false);

console.log("module core api passed");
