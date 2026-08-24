'use strict';

const assert = require('assert');
const { Writable } = require('stream');

const views = [new Uint8Array([65]), new Int8Array([66]), new Uint8ClampedArray([67])];
let firstCallback;
const writable = new Writable({
  write(chunk, encoding, callback) {
    assert(chunk instanceof Buffer);
    assert.strictEqual(encoding, 'buffer');
    firstCallback = callback;
  },
  writev(chunks, callback) {
    assert.strictEqual(chunks.length, views.length);
    assert(chunks.every((entry) => entry.encoding === 'buffer'));
    assert.strictEqual(chunks.map((entry) => entry.chunk.toString()).join(''), 'BCA');
  },
});

for (const view of views) writable.write(view);
writable.end(views[0]);
firstCallback();

console.log('PASS stream typed-array writev');
