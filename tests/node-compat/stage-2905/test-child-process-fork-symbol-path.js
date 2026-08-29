'use strict';

const assert = require('node:assert');
const { fork } = require('node:child_process');

assert.throws(() => fork(Symbol('module')), {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
});
