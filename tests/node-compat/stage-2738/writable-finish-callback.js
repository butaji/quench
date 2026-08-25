const assert = require('assert');
const { Writable } = require('stream');
const writable = new Writable();
writable._write = (_chunk, _encoding, callback) => process.nextTick(callback);
let called = false;
writable.end('asd', (error) => { called = true; assert.strictEqual(error, null); });
writable.on('error', (error) => assert.strictEqual(error.message, 'kaboom'));
writable.on('finish', () => {
  assert.strictEqual(called, true);
  writable.emit('error', new Error('kaboom'));
});
