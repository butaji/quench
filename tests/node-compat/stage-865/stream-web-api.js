"use strict";

const assert = require("assert");
const web = require("node:stream/web");

for (
  const name of [
    "ReadableStream",
    "WritableStream",
    "TransformStream",
    "ReadableStreamDefaultReader",
    "WritableStreamDefaultWriter",
    "ByteLengthQueuingStrategy",
    "CountQueuingStrategy",
  ]
) {
  assert.strictEqual(typeof web[name], "function");
}
assert.strictEqual(typeof web.ReadableStream.from, "function");
assert.strictEqual(typeof web.ReadableStream.prototype.getReader, "function");

console.log("stream web api passed");
