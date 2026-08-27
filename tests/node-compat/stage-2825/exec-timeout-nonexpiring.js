const assert = require('assert');
const { exec } = require('child_process');

exec('sleep 2m', { timeout: 1e9 }, (error, stdout, stderr) => {
  assert.ifError(error);
  assert.strictEqual(stdout, 'child stdout\n');
  assert.strictEqual(stderr, 'child stderr\n');
});
