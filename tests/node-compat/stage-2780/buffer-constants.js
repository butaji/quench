'use strict';

const assert = require('assert');
const buffer = require('buffer');

const { MAX_LENGTH, MAX_STRING_LENGTH } = buffer.constants;
assert.strictEqual(typeof MAX_LENGTH, 'number');
assert.strictEqual(typeof MAX_STRING_LENGTH, 'number');
assert(MAX_STRING_LENGTH <= MAX_LENGTH);
assert.strictEqual(buffer.kMaxLength, MAX_LENGTH);
assert.strictEqual(buffer.kStringMaxLength, MAX_STRING_LENGTH);
assert.throws(
  () => ' '.repeat(MAX_STRING_LENGTH + 1),
  /^RangeError: Invalid string length$/,
);
' '.repeat(MAX_STRING_LENGTH);
