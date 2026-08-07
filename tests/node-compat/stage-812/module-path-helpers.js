"use strict";

const assert = require("assert");
const Module = require("module").Module;

for (
  const name of [
    "_nodeModulePaths",
    "_findPath",
    "_resolveLookupPaths",
    "_load",
  ]
) {
  assert.strictEqual(typeof Module[name], "function");
}
assert.strictEqual(Array.isArray(Module._nodeModulePaths(process.cwd())), true);

console.log("module path helpers passed");
