'use strict';
const assert = require('assert');
const { Readable } = require('stream');

const readable = new Readable({ read() {} });
assert.strictEqual(readable._readableState.emittedReadable, false);
readable.on('readable', () => {
  assert.strictEqual(readable._readableState.emittedReadable, true);
  readable.read();
  assert.strictEqual(readable._readableState.emittedReadable, false);
});
readable.push('value');
readable.push(null);
