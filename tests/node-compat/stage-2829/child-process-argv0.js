const assert = require('assert');
const cp = require('child_process');

const base = cp.spawnSync(process.execPath, ['script.js', 'child']);
assert.strictEqual(base.stdout.toString().trim(), process.execPath);
const custom = cp.spawnSync(process.execPath, ['script.js', 'child'], {
  argv0: 'custom'
});
assert.strictEqual(custom.stdout.toString().trim(), 'custom');
assert.throws(() => cp.spawnSync(process.execPath, ['child'], { argv0: [] }), {
  code: 'ERR_INVALID_ARG_TYPE'
});
