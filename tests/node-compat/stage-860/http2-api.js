"use strict";

const assert = require("assert");
const http2 = require("node:http2");

for (const name of ["connect", "createServer", "createSecureServer"]) {
  assert.strictEqual(typeof http2[name], "function");
}
for (
  const name of [
    "Http2Server",
    "Http2SecureServer",
    "Http2Session",
    "Http2Stream",
  ]
) {
  assert.strictEqual(typeof http2[name], "function");
}
assert.strictEqual(typeof http2.constants, "object");
assert.strictEqual(typeof http2.getDefaultSettings, "function");

console.log("http2 api passed");
