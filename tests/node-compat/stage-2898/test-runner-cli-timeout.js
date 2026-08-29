const assert = require('assert');
const { spawnSync } = require('child_process');

const defaultRun = spawnSync(process.execPath, ['--test'], {
  env: { ...process.env, NODE_DEBUG: 'test_runner' },
});
assert.match(defaultRun.stderr.toString(), /timeout: Infinity,/);

const timedRun = spawnSync(process.execPath, ['--test', '--test-timeout', 10], {
  env: { ...process.env, NODE_DEBUG: 'test_runner' },
});
assert.match(timedRun.stderr.toString(), /timeout: 10,/);
