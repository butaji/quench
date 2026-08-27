const assert = require('assert');
const { test } = require('node:test');

assert.throws(() => test({ signal: {} }), { code: 'ERR_INVALID_ARG_TYPE', name: 'TypeError' });
