const assert = require('assert');
const { test } = require('node:test');

test('mock.module validates its declaration facts', (t) => {
  assert.throws(() => t.mock.module(5), { code: 'ERR_INVALID_ARG_TYPE' });
  assert.throws(() => t.mock.module('x', null), { code: 'ERR_INVALID_ARG_TYPE' });
  assert.throws(() => t.mock.module('x', { cache: 5 }), { code: 'ERR_INVALID_ARG_TYPE' });
  assert.throws(() => t.mock.module('x', { exports: {}, namedExports: {} }), { code: 'ERR_INVALID_ARG_VALUE' });
});
