'use strict';

const assert = require('assert');
const util = require('util');

const values = Uint8Array.of(0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
assert.strictEqual(values.byteLength, 10);
assert.deepStrictEqual(Array.from(values), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

const target = Uint8Array.of(0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
assert.strictEqual(Buffer.prototype.compare.call(values, target, 0, 4, 0, 4), 0);
assert.strictEqual(Buffer.prototype.compare.call(values, target, 0, 4, 1, 5), 1);

const destination = new Uint8Array(4);
Buffer.prototype.copy.call(values, destination, 1, 7, 10);
assert.deepStrictEqual(Array.from(destination), [0, 7, 8, 9]);

assert.strictEqual(util.inspect(Buffer.from('fhqwhgads')), '<Buffer 66 68 71 77 68 67 61 64 73>');
assert.match(util.inspect(Buffer.from('x'.repeat(51))), /^<Buffer (?:78 ){50}\.\.\. 1 more byte>$/);
