const assert = require("assert");
const { Buffer } = require("buffer");

const buffer = Buffer.from([1, 2, 3, 4]);
const view = buffer.subarray(1, 3);
assert.strictEqual(typeof view.swap16, "function");
assert.strictEqual(typeof view.copy, "function");
view.swap16();
assert.deepStrictEqual(Array.from(buffer), [1, 3, 2, 4]);

const copy = buffer.slice(1, 3);
assert.strictEqual(typeof copy.swap16, "function");
copy[0] = 9;
assert.strictEqual(buffer[1], 9);
