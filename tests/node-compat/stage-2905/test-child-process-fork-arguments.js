'use strict';

const assert = require('node:assert');
const { fork } = require('node:child_process');

for (const args of [0, true, () => {}, Symbol('test')]) {
  assert.throws(() => fork('child.js', args), { code: 'ERR_INVALID_ARG_TYPE' });
}
for (const options of [0, true, () => {}, Symbol('test')]) {
  assert.throws(() => fork('child.js', [], options), { code: 'ERR_INVALID_ARG_TYPE' });
}
