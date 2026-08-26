'use strict';
const assert = require('assert');

let seen = 0;
process.emitWarning('late listener', { code: 'TEST_WARNING' });
process.once('warning', (warning) => {
  seen += 1;
  assert.strictEqual(warning.name, 'Warning');
  assert.strictEqual(warning.code, 'TEST_WARNING');
  assert.strictEqual(warning.message, 'late listener');
});

setImmediate(() => assert.strictEqual(seen, 1));
