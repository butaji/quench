const assert = require("assert");
const { Buffer } = require("node:buffer");
const value = Buffer.from("hello");
assert.strictEqual(Buffer.isBuffer(value), true);
assert.strictEqual(Buffer.byteLength("hello"), 5);
assert.strictEqual(
  Buffer.concat([Buffer.from("hel"), Buffer.from("lo")]).toString(),
  "hello",
);
assert.strictEqual(Buffer.from("hello").toString("base64"), "aGVsbG8=");
assert.strictEqual(Buffer.from("aGVsbG8=", "base64").toString(), "hello");
