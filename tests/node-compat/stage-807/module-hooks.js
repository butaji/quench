"use strict";

const assert = require("assert");
const moduleApi = require("module");

for (
  const name of [
    "registerHooks",
    "flushCompileCache",
    "getCompileCacheDir",
  ]
) {
  assert.strictEqual(typeof moduleApi[name], "function");
}

console.log("module hooks passed");
