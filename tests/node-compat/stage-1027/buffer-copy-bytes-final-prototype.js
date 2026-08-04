const assert = require("assert");
const { Buffer } = require("buffer");

const values = new Uint16Array([0xffff]);
const buffer = Buffer.copyBytesFrom(values);
assert.strictEqual(buffer.constructor.name, "NodeBuffer");
assert.strictEqual(typeof buffer.copy, "function");
assert.deepStrictEqual(Array.from(buffer), [255, 255]);

values[0] = 0;
assert.deepStrictEqual(Array.from(buffer), [255, 255]);
