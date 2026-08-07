"use strict";

const assert = require("assert");
const v8 = require("node:v8");

for (
  const name of [
    "serialize",
    "deserialize",
    "getHeapStatistics",
    "getHeapSpaceStatistics",
    "getHeapCodeStatistics",
    "setFlagsFromString",
    "cachedDataVersionTag",
  ]
) {
  assert.strictEqual(typeof v8[name], "function");
}
assert.strictEqual(typeof v8.cachedDataVersionTag(), "number");

console.log("v8 api passed");
