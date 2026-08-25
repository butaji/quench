const assert = require('assert');
const { Writable } = require('stream');

for (const autoDestroy of [false, true]) {
  const stream = new Writable({ autoDestroy, write() {} });
  stream.end();
  let callbackError;
  let eventError;
  stream.write('late', (error) => { callbackError = error; });
  stream.on('error', (error) => { eventError = error; });
  process.nextTick(() => process.nextTick(() => {
    assert.strictEqual(callbackError.code, 'ERR_STREAM_WRITE_AFTER_END');
    assert.strictEqual(eventError.code, 'ERR_STREAM_WRITE_AFTER_END');
  }));
}
