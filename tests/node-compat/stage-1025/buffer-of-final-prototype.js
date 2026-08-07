const assert = require("assert");
const { Buffer } = require("buffer");

const buffer = Buffer.of(1, 2, 3, 4);
assert.strictEqual(buffer.constructor.name, "NodeBuffer");
assert.strictEqual(typeof buffer.copy, "function");
assert.deepStrictEqual(Array.from(buffer), [1, 2, 3, 4]);

const target = new Uint16Array(4);
assert.strictEqual(buffer.copy(target), 4);
assert.deepStrictEqual(
  Array.from(new Uint8Array(target.buffer, target.byteOffset, 4)),
  [1, 2, 3, 4],
);
