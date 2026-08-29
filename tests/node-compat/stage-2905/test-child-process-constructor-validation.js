'use strict';

const assert = require('node:assert');
const { ChildProcess } = require('node:child_process');

const child = new ChildProcess();
for (const options of [undefined, null, 'foo', 0, true]) {
  assert.throws(() => child.spawn(options), { code: 'ERR_INVALID_ARG_TYPE' });
}
for (const args of [null, 0, true, {}]) {
  assert.throws(() => child.spawn({ file: 'foo', args }), { code: 'ERR_INVALID_ARG_TYPE' });
}
