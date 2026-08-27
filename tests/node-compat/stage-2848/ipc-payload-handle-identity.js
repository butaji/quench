'use strict';

const assert = require('assert');
const { fork } = require('child_process');

if (process.argv[2] === 'child') {
  process.send({ what: 'ready' });
  process.on('message', (message, handle) => {
    assert.strictEqual(message.kind, 'payload');
    assert.strictEqual(handle.tag, 'shared');
  });
} else {
  const child = fork(__filename, ['child']);
  child.on('message', (message) => {
    if (message.what === 'ready') {
      child.send({ kind: 'payload' }, { tag: 'shared' });
    }
  });
}
