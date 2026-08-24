const assert = require('node:assert');
const { Writable } = require('node:stream');

for (const autoDestroy of [false, true]) {
  const writable = new Writable({ autoDestroy, write() {} });
  writable.end();
  let callbackCalled = false;
  let errorCalled = false;
  writable.write('data', (error) => {
    assert.strictEqual(error.code, 'ERR_STREAM_WRITE_AFTER_END');
    assert.strictEqual(errorCalled, false);
    callbackCalled = true;
  });
  writable.on('error', (error) => {
    assert.strictEqual(error.code, 'ERR_STREAM_WRITE_AFTER_END');
    errorCalled = true;
  });
  setTimeout(() => {
    assert.strictEqual(callbackCalled, true);
    assert.strictEqual(errorCalled, true);
  }, 10);
}
console.log('stream writable write-after-end: ok');
