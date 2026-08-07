"use strict";

const assert = require("assert");
const vm = require("node:vm");

for (
  const name of [
    "runInContext",
    "runInNewContext",
    "runInThisContext",
    "createContext",
    "isContext",
    "compileFunction",
  ]
) {
  assert.strictEqual(typeof vm[name], "function");
}
for (
  const name of [
    "Script",
    "Context",
    "Module",
    "SourceTextModule",
    "SyntheticModule",
  ]
) {
  assert.strictEqual(typeof vm[name], "function");
}

console.log("vm api passed");
