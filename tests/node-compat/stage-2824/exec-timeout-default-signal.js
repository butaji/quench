const assert = require('assert');
const { exec } = require('child_process');

exec('sleep 2m', { timeout: 1 }, (error, stdout, stderr) => {
  assert.strictEqual(error.killed, true);
  assert.strictEqual(error.code, null);
  assert.strictEqual(error.signal, 'SIGTERM');
  assert.strictEqual(stdout, '');
  assert.strictEqual(stderr, '');
});
