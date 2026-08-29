'use strict';

const assert = require('assert');
const { MessageChannel } = require('worker_threads');

const pooled = Buffer.from('hello world');
const sibling = Buffer.from('hello world');
assert.strictEqual(pooled.buffer, sibling.buffer);
const { port1 } = new MessageChannel();
assert.throws(() => port1.postMessage(pooled, [pooled.buffer]), {
  name: 'DataCloneError',
  code: 25,
});
assert.strictEqual(pooled.buffer, sibling.buffer);
assert.strictEqual(pooled.length, 11);
assert.throws(() => pooled.buffer.transfer(), TypeError);

const transferable = new ArrayBuffer(2);
assert.doesNotThrow(() => port1.postMessage(new Uint8Array(transferable), [transferable]));
