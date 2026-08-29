'use strict';

const assert = require('node:assert');
const { fork } = require('node:child_process');

for (const modulePath of [0, true, undefined, null, [], {}, () => {}]) {
  assert.throws(
    () => fork(modulePath),
    { code: 'ERR_INVALID_ARG_TYPE', name: 'TypeError' },
  );
}
