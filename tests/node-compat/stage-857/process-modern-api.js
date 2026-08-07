"use strict";

const assert = require("assert");
const processApi = require("node:process");

for (
  const name of [
    "availableMemory",
    "constrainedMemory",
    "getActiveResourcesInfo",
    "finalization",
  ]
) {
  assert.ok(name in processApi);
}
assert.strictEqual(typeof processApi.availableMemory, "function");
assert.strictEqual(typeof processApi.constrainedMemory, "function");
assert.strictEqual(typeof processApi.getActiveResourcesInfo, "function");
assert.strictEqual(typeof processApi.finalization, "object");

console.log("modern process api passed");
