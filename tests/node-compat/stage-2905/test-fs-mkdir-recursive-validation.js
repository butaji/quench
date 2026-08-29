'use strict';

const assert = require('node:assert');
const fs = require('node:fs');

for (const recursive of ['', 1, {}, []]) {
  assert.throws(
    () => fs.mkdirSync('node-compat-invalid-mkdir', { recursive }),
    { code: 'ERR_INVALID_ARG_TYPE', name: 'TypeError' },
  );
}
