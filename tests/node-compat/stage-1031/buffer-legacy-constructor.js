const assert = require("assert");
const { Buffer } = require("buffer");

const empty = new Buffer(0);
assert.ok(empty.parent instanceof ArrayBuffer);
assert.strictEqual(empty.length, 0);

const allocated = new Buffer(4);
assert.strictEqual(allocated.length, 4);
assert.strictEqual(typeof allocated.copy, "function");

const arrayBuffer = new ArrayBuffer(0);
assert.strictEqual(new Buffer(arrayBuffer).buffer, arrayBuffer);
