const assert = require('assert');
const { exec } = require('child_process');

exec('sleep 2m', { timeout: 1, killSignal: 'SIGKILL' }, (error, stdout, stderr) => {
  assert.strictEqual(error.killed, true);
  assert.strictEqual(error.code, null);
  assert.strictEqual(error.signal, 'SIGKILL');
  assert.strictEqual(stdout, '');
  assert.strictEqual(stderr, '');
});
