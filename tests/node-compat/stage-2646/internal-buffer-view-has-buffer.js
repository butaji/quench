"use strict";
const assert = require("assert");
const { internalBinding } = require("internal/test/binding");
const { arrayBufferViewHasBuffer } = internalBinding("util");

for (const [view, expected] of [
  [new Uint8Array(48), false],
  [new Uint8Array(96), true],
  [Buffer.alloc(48), false],
  [Buffer.alloc(96), true],
]) {
  assert.strictEqual(arrayBufferViewHasBuffer(view), expected);
  view.buffer;
  assert.strictEqual(arrayBufferViewHasBuffer(view), true);
}
