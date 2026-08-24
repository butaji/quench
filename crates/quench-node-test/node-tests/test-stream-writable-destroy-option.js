const assert = require('node:assert');
const { Writable } = require('node:stream');

const expected = new Error('destroyed');
let received;
let closed = false;
let repeatCallback = false;
const writable = new Writable({
  destroy(error, callback) {
    received = error;
    callback();
  }
});
writable.on('close', () => { closed = true; });
writable.destroy(expected);
assert.strictEqual(writable.destroyed, true);
writable.destroy(expected, () => { repeatCallback = true; });
setTimeout(() => {
  assert.strictEqual(received, expected);
  assert.strictEqual(closed, true);
  assert.strictEqual(repeatCallback, true);
  console.log('stream writable destroy option: ok');
}, 10);
