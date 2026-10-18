'use strict';
const assert = require('node:assert/strict');
const cp = require('node:child_process');
const { mock } = require('node:test');
const original = cp.ChildProcess.prototype.spawn;
const patched = mock.method(cp.ChildProcess.prototype, 'spawn', function() {
  const result = original.apply(this, arguments);
  this.stdout.destroy(); this.stderr.destroy();
  this.stdout = null; this.stderr = null;
  return result;
});
let callbacks = 0;
const injected = new Error('injected child error');
const child = cp.exec('while :; do :; done', (error, stdout, stderr) => {
  callbacks++;
  assert.equal(error, injected);
  assert.equal(stdout, ''); assert.equal(stderr, '');
});
child.on('close', () => {
  assert.equal(callbacks, 1);
  assert.equal(patched.mock.callCount(), 1);
  mock.restoreAll();
  console.log('CHILD_PROCESS_NULL_STDIO_ERROR_OK');
});
child.emit('error', injected);
child.kill();
