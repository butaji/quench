const assert = require("assert");
const { Buffer } = require("buffer");

const source = new Uint8Array([1, 2, 3, 4]);
const shared = Buffer.from(source.buffer);
shared[1] = 9;
assert.strictEqual(source[1], 9);

const sub = shared.subarray(1, 3);
sub[0] = 8;
assert.strictEqual(shared[1], 8);
const sliced = shared.slice(1, 3);
sliced[0] = 7;
assert.strictEqual(shared[1], 7);

const copied = Buffer.alloc(2);
shared.copy(copied, 0, 1, 3);
shared[1] = 6;
assert.strictEqual(copied[0], 7);
assert.deepStrictEqual(
  Buffer.concat([Buffer.from([1]), Buffer.from([2, 3])]),
  Buffer.from([1, 2, 3]),
);
