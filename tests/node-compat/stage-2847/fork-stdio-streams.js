'use strict';

const assert = require('assert');
const { fork } = require('child_process');

if (process.argv[2] === 'child') {
  process.stderr.write('fork stderr');
} else {
  const child = fork(__filename, ['child'], {
    stdio: [0, 'ignore', 'pipe', 'ipc', 'pipe'],
  });
  assert.strictEqual(child.stdout, null);
  let stderr = '';
  child.stderr.on('data', (chunk) => { stderr += chunk; });
  child.on('exit', (code) => {
    assert.strictEqual(code, 0);
    assert.strictEqual(stderr, 'fork stderr');
  });
}
