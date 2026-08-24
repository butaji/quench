const assert = require('node:assert');
const { Writable } = require('node:stream');

const error = new Error('write failed');
let callbackCount = 0;
let errorCount = 0;
const writable = new Writable({
  write(_chunk, _encoding, callback) {
    process.nextTick(callback, error);
  }
});
writable.on('error', (received) => {
  assert.strictEqual(received, error);
  errorCount += 1;
});
writable.write('data');
writable.end((received) => {
  assert.strictEqual(received, error);
  callbackCount += 1;
});
writable.end((received) => {
  assert.strictEqual(received, error);
  callbackCount += 1;
});
setTimeout(() => {
  assert.strictEqual(callbackCount, 2);
  assert.strictEqual(errorCount, 1);
  console.log('stream writable errors: ok');
}, 10);
