const assert = require('assert');
const { execFile } = require('child_process');
const ac = new AbortController();
let seen;
execFile(process.execPath, ['echo.js', 0], { signal: ac.signal }, (err) => { seen = err; });
assert.strictEqual(require('events').listenerCount(ac.signal, 'abort'), 1);
ac.abort();
assert.strictEqual(ac.signal.aborted, true);
setImmediate(() => {
  assert.ok(seen);
  assert.strictEqual(seen.code, 'ABORT_ERR');
  assert.strictEqual(seen.name, 'AbortError');
});

const success = new AbortController();
execFile(process.execPath, ['echo.js', 0], { signal: success.signal }, (err) => {
  assert.strictEqual(err, null);
  assert.strictEqual(require('events').listenerCount(success.signal, 'abort'), 0);
});
