const assert = require('node:assert');
const { Writable } = require('node:stream');

const expected = new Error('destroyed');
let received;
let closed = false;
const writable = new Writable({
  destroy(error, callback) {
    received = error;
    callback();
  }
});
writable.on('close', () => { closed = true; });
writable.destroy(expected);
assert.strictEqual(writable.destroyed, true);
setTimeout(() => {
  assert.strictEqual(received, expected);
  assert.strictEqual(closed, true);
  console.log('stream writable destroy option: ok');
}, 10);
