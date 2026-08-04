const assert = require("assert");
const { Buffer } = require("buffer");

const first = Buffer.from("hello world");
const second = Buffer.from("hello world");
assert.strictEqual(first.buffer, second.buffer);
assert.strictEqual(first.toString(), "hello world");
assert.strictEqual(second.toString(), "hello world");
