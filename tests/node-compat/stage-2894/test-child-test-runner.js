const assert = require('assert');
const { spawnSync } = require('child_process');

if (process.argv[2] === 'child') {
  process.stdout.write('child ok\n');
} else {
  const result = spawnSync(process.execPath, ['--test', __filename, 'child']);
  assert.strictEqual(result.status, 0);
  assert.match(result.stdout.toString(), /child ok/);
}
