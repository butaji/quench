"use strict";

const assert = require("assert");
const http2 = require("node:http2");

for (
  const name of [
    "getPackedSettings",
    "getUnpackedSettings",
    "sensitiveHeaders",
  ]
) {
  assert.strictEqual(typeof http2[name], "function");
}
assert.strictEqual(typeof http2.constants, "object");

console.log("http2 settings api passed");
