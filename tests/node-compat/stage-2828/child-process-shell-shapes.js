const assert = require('assert');
const cp = require('child_process');

const child = cp.spawn('echo', ['foo'], { shell: true });
assert.strictEqual(child.spawnargs.at(-1), 'echo foo');
child.stdout.on('data', (value) => assert.strictEqual(value.toString().trim(), 'foo'));

const missing = cp.spawn('does-not-exist', { shell: true });
missing.on('exit', (code, signal) => {
  assert.strictEqual(code, 127);
  assert.strictEqual(signal, null);
});
