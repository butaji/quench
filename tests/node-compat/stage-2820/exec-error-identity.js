const assert = require('assert');
const { exec, execFile } = require('child_process');

const shellChild = exec('does-not-exist', (error) => {
  assert.strictEqual(error.code, 127);
  assert.ok(error.cmd.includes('does-not-exist'));
});
assert.strictEqual(typeof shellChild.pid, 'number');

const fileChild = execFile('does-not-exist', (error) => {
  assert.strictEqual(error.code, 'ENOENT');
  assert.ok(error.cmd.includes('does-not-exist'));
});
assert.strictEqual(typeof fileChild.pid, 'undefined');
