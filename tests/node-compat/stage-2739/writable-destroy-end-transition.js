const assert = require('assert');
const { Writable } = require('stream');

const destroyed = new Writable({ write(_chunk, _encoding, callback) { callback(); } });
destroyed.destroy();
let destroyedError;
destroyed.end((error) => { destroyedError = error; });

const expected = new Error('pending');
const pending = new Writable({
  write(_chunk, _encoding, callback) { process.nextTick(callback); },
});
let pendingError;
pending.on('error', () => {});
pending.end((error) => { pendingError = error; });
pending.destroy(expected);

process.nextTick(() => process.nextTick(() => {
  assert.strictEqual(destroyedError.code, 'ERR_STREAM_DESTROYED');
  assert.strictEqual(pendingError, expected);
}));
