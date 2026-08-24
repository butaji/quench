'use strict';

const assert = require('assert');
const { Buffer } = require('buffer');

const buffer = Buffer.allocUnsafe(8);
assert.strictEqual(buffer.writeBigInt64LE(123n, 0), 8);
assert.strictEqual(buffer.readBigInt64LE(0), 123n);
assert.strictEqual(buffer.writeUInt32BE(0x12345678, 0), 4);
assert.strictEqual(buffer.readUInt32BE(0), 0x12345678);

console.log('PASS buffer numeric method receiver');
