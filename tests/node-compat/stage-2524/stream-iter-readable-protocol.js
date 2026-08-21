const assert = require("assert");
const { Readable } = require("stream");
const { from } = require("stream/iter");

const readable = new Readable({ read() {} });
const protocol = Symbol.for("Stream.toAsyncStreamable");
const source = readable[protocol]();
assert.strictEqual(source.stream, readable);
assert.strictEqual(typeof source[Symbol.asyncIterator], "function");
assert.strictEqual(from(source), source);
