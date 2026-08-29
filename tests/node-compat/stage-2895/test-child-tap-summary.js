const assert = require('assert');
const { spawnSync } = require('child_process');

if (process.argv[2] === 'child') {
  const { test } = require('node:test');
  test('child', () => {});
} else {
  const result = spawnSync(process.execPath, ['--test', __filename, 'child']);
  const output = result.stdout.toString();
  assert.strictEqual(result.status, 0);
  assert.match(output, /# pass 1/);
  assert.match(output, /# fail 0/);
}
