'use strict';
const assert = require('assert');
const { Readable } = require('stream');
const common = require('../../node/test/common');
const calls = [];
const values = ['a', null];
const readable = new Readable({
  objectMode: true,
  highWaterMark: 0,
  read: common.mustCall(function readCallback() {
    calls.push('_read:' + values[0]);
    process.nextTick(() => {
      calls.push('push:' + values[0]);
      readable.push(values.shift());
    });
  }, 2)
});
readable.on('readable', common.mustCall(function readableCallback() { calls.push('readable'); }, 2));
readable.on('data', common.mustCall(function dataCallback(value) { calls.push('data:' + value); }, 1));
readable.on('end', common.mustCall(function endCallback() { calls.push('end'); }));
setImmediate(common.mustCall(function outerImmediate() {
  assert.deepStrictEqual(calls, ['_read:a', 'push:a', 'readable']);
  assert.strictEqual(readable.read(), 'a');
  assert.deepStrictEqual(calls, ['_read:a', 'push:a', 'readable', 'data:a', '_read:null']);
  assert.strictEqual(readable.read(), null);
  setImmediate(common.mustCall(function innerImmediate() {
    assert.deepStrictEqual(calls, ['_read:a', 'push:a', 'readable', 'data:a', '_read:null', 'push:null', 'readable']);
    assert.strictEqual(readable.read(), null);
    process.nextTick(() => assert.strictEqual(calls[calls.length - 1], 'end'));
  }));
}));
