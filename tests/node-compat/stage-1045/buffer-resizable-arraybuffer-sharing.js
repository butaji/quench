const assert = require("assert");
const { Buffer } = require("buffer");

const arrayBuffer = new ArrayBuffer(10, { maxByteLength: 20 });
const buffer = Buffer.from(arrayBuffer, 1);
assert.strictEqual(buffer.byteLength, 9);
arrayBuffer.resize(15);
assert.strictEqual(buffer.byteLength, 14);
arrayBuffer.resize(5);
assert.strictEqual(buffer.byteLength, 4);
