const assert = require('assert');
const { promisify } = require('util');
const { execFile } = require('child_process');
const run = promisify(execFile);

const controller = new AbortController();
const pending = run(process.execPath, ['echo.js', 0], { signal: controller.signal });
controller.abort();
assert.rejects(pending, { name: 'AbortError' });

assert.throws(
  () => run(process.execPath, ['echo.js'], { signal: {} }),
  { name: 'TypeError', code: 'ERR_INVALID_ARG_TYPE' },
);
