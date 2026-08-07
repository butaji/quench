"use strict";

const assert = require("assert");
const bufferApi = require("node:buffer");

assert.strictEqual(typeof bufferApi.Buffer, "function");
assert.strictEqual(typeof bufferApi.Buffer.from, "function");
assert.strictEqual(typeof bufferApi.Buffer.alloc, "function");
assert.strictEqual(typeof bufferApi.Buffer.isBuffer, "function");
assert.strictEqual(typeof bufferApi.isUtf8, "function");
assert.strictEqual(typeof bufferApi.isAscii, "function");
assert.strictEqual(
  bufferApi.Buffer.isBuffer(bufferApi.Buffer.from("ok")),
  true,
);

console.log("buffer core api passed");
