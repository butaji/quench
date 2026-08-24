'use strict';

const assert = require('assert');
const buffer = Buffer.from('abc');

assert.strictEqual(typeof buffer.map, 'function');
assert.strictEqual(typeof buffer.filter, 'function');
assert.deepStrictEqual([...buffer.map((value) => value + 1)], [98, 99, 100]);
assert.deepStrictEqual([...buffer.filter((value) => value !== 98)], [97, 99]);
