'use strict';

const assert = require('node:assert');
const { ChildProcess } = require('node:child_process');

const child = new ChildProcess();
assert.throws(() => child.kill('foo'), {
  code: 'ERR_UNKNOWN_SIGNAL',
  name: 'TypeError',
});
