'use strict';

const assert = require('assert');
const { fork } = require('child_process');

assert.throws(
  () => fork(process.execPath, { stdio: ['pipe', 'pipe', 'pipe', 'pipe'] }),
  { code: 'ERR_CHILD_PROCESS_IPC_REQUIRED', name: 'Error' },
);
