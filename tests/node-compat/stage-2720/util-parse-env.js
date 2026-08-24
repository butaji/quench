'use strict';

const assert = require('assert');
const { parseEnv } = require('util');

assert.deepStrictEqual(parseEnv('export A="B=C"\nB=plain # comment\n'), {
  __proto__: null,
  A: 'B=C',
  B: 'plain',
});
assert.deepStrictEqual(parseEnv('MULTI="one\ntwo"\nEMPTY=\n'), {
  __proto__: null,
  MULTI: 'one\ntwo',
  EMPTY: '',
});
assert.throws(() => parseEnv(null), { code: 'ERR_INVALID_ARG_TYPE' });
