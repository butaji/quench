'use strict';
// Run unchanged with Node26 and the qualified Quench binary. Genuine OS children
// execute only /bin/sh builtins; node:test supplies its original SDK MockTracker.
const assert = require('node:assert/strict');
const cp = require('node:child_process');
const { mock } = require('node:test');
const { ChildProcess } = require('internal/child_process');
assert.equal(ChildProcess, cp.ChildProcess);
const descriptor = Object.getOwnPropertyDescriptor(ChildProcess.prototype, 'spawn');
assert.equal(typeof descriptor.value, 'function');
assert.equal(descriptor.value.name, 'spawn');
assert.equal(descriptor.value.length, 1);
assert.equal(descriptor.writable, true);
assert.equal(descriptor.enumerable, true);
assert.equal(descriptor.configurable, true);
const original = descriptor.value;
const events = [];
const patched = mock.method(ChildProcess.prototype, 'spawn', function(options) {
  events.push('intercept');
  const result = original.apply(this, arguments);
  assert.equal(result, 0);
  assert.equal(typeof this.pid, 'number');
  assert.ok(this.pid > 0);
  assert.equal(this.spawnfile, options.file);
  assert.equal(this.spawnargs, options.args);
  this.stdout.destroy();
  this.stderr.destroy();
  this.stdout = null;
  this.stderr = null;
  return result;
});
// Stream destruction races OS writes under both runtimes. Keep the real child
// successful independently of when the intentionally closed pipes are retired.
const child = cp.exec('printf child-output >/dev/null', (error, stdout, stderr) => {
  assert.equal(error, null);
  assert.equal(stdout, '');
  assert.equal(stderr, '');
  events.push('callback');
});
assert.equal(patched.mock.callCount(), 1);
assert.equal(patched.mock.calls[0].this, child);
assert.equal(patched.mock.calls[0].result, 0);
assert.equal(patched.mock.calls[0].error, undefined);
child.on('spawn', () => events.push('spawn'));
child.on('exit', () => events.push('exit'));
child.on('close', () => {
  events.push('close');
  assert.deepEqual(events, ['intercept', 'spawn', 'exit', 'callback', 'close']);
  mock.restoreAll();
  assert.equal(ChildProcess.prototype.spawn, original);
  console.log('CHILD_PROCESS_MOCKTRACKER_OK');
});
