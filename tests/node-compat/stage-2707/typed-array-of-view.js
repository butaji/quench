const assert = require("assert");
const direct = new Uint8Array(4);
assert.strictEqual(direct.length, 4);
const from = Uint8Array.from([0, 1, 2, 3]);
assert.strictEqual(from.length, 4);
const value = Uint8Array.of(0, 1, 2, 3);
assert.strictEqual(value.length, 4);
assert.strictEqual(value.byteLength, 4);
assert.deepStrictEqual(Array.from(value), [0, 1, 2, 3]);
