"use strict";

const assert = require("assert");
const moduleApi = require("module");

for (
  const name of [
    "register",
    "runMain",
    "findPackageJSON",
    "getSourceMapsSupport",
    "setSourceMapsSupport",
    "stripTypeScriptTypes",
    "enableCompileCache",
  ]
) {
  assert.strictEqual(typeof moduleApi[name], "function");
}
assert.strictEqual(
  typeof moduleApi.stripTypeScriptTypes("const value = 1"),
  "string",
);

console.log("module modern api passed");
