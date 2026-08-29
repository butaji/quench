const assert = require('assert');
const child = require('child_process').spawnSync(process.execPath, ['script.js', 'child'], {
  env: { ...process.env, foo: 'bar' }
});

assert.strictEqual(child.stdout.toString().trim(), 'bar');
assert.strictEqual(child.status, 0);
