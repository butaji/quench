const assert = require('assert');
const { Writable } = require('stream');

let close = 0;
const stream = new Writable({ write(_chunk, _encoding, callback) { callback(); } });
stream.on('close', () => { close++; });
stream.destroy();
assert.strictEqual(stream.destroyed, true);
process.nextTick(() => assert.strictEqual(close, 1));

let destroyError;
const expected = new Error('stage');
const custom = new Writable({
  write(_chunk, _encoding, callback) { callback(); },
  destroy(error, callback) {
    destroyError = error;
    callback(error);
  },
});
let seen;
custom.on('error', (error) => { seen = error; });
custom.destroy(expected);
assert.strictEqual(custom.destroyed, true);
process.nextTick(() => {
  assert.strictEqual(destroyError, expected);
  assert.strictEqual(seen, expected);
});
