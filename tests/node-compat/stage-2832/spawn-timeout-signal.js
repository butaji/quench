const assert = require('assert');
const cp = require('child_process');

assert.throws(() => cp.spawn(process.execPath, [], { timeout: 'bad' }), {
  code: 'ERR_INVALID_ARG_TYPE'
});
const child = cp.spawn(process.execPath, ['stay-alive'], {
  timeout: 1,
  killSignal: 'SIGKILL'
});
child.on('exit', (code, signal) => {
  assert.strictEqual(code, null);
  assert.strictEqual(signal, 'SIGKILL');
});
