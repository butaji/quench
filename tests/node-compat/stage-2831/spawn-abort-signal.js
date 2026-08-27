const assert = require('assert');
const childProcess = require('child_process');

const controller = new AbortController();
const child = childProcess.spawn(process.execPath, ['stay-alive'], {
  signal: controller.signal
});
let error;
child.on('error', (value) => { error = value; });
child.on('exit', (code, signal) => {
  assert.strictEqual(code, null);
  assert.strictEqual(signal, 'SIGTERM');
  assert.strictEqual(error.name, 'AbortError');
});
controller.abort();
