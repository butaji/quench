const assert = require('assert');

const values = new Uint16Array([0x1122, 0x3344]);
const all = Buffer.copyBytesFrom(values);
assert.strictEqual(all.length, 4);
assert.strictEqual(all[0], 0x22);
assert.strictEqual(all[1], 0x11);
assert.strictEqual(all[2], 0x44);
assert.strictEqual(all[3], 0x33);
const tail = Buffer.copyBytesFrom(values, 1);
assert.strictEqual(tail.length, 2);
assert.strictEqual(tail[0], 0x44);
assert.strictEqual(tail[1], 0x33);
const first = Buffer.copyBytesFrom(values, 0, 1);
assert.strictEqual(first.length, 1);
assert.strictEqual(first[0], 0x22);
console.log('buffer copyBytesFrom view: ok');
