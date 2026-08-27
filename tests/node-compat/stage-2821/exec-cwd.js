const assert = require('assert');
const { exec } = require('child_process');

exec('pwd', { cwd: '/dev' }, (error, stdout, stderr) => {
  assert.ifError(error);
  assert.strictEqual(stderr, '');
  assert.ok(stdout.toLowerCase().startsWith('/dev'));
});
