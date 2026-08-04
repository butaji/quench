"use strict";

const assert = require("assert");
const extensions = require("module").Module._extensions;

for (const extension of [".js", ".json", ".node"]) {
  assert.strictEqual(typeof extensions[extension], "function");
}

console.log("module extensions passed");
