'use strict';

const assert = require('assert');
const text = Array.from({ length: 65536 }, (_, index) => String.fromCharCode(index)).join('');
const bytes = Buffer.from(text);
const offset = 0x2f77b;
const needle = bytes.slice(offset, offset + 60);

assert.strictEqual(Buffer.byteLength(text), 194430);
assert.strictEqual(bytes.length, 194430);
assert.strictEqual(bytes.indexOf(needle.toString()), offset);
