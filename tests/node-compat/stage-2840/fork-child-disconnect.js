const assert = require('assert');
const { fork } = require('child_process');

if (process.argv[2] === 'child') {
  process.disconnect();
} else {
  const child = fork(__filename, ['child']);
  child.once('disconnect', () => {
    child.once('exit', (code, signal) => {
      assert.strictEqual(code, 0);
      assert.strictEqual(signal, null);
    });
  });
}
