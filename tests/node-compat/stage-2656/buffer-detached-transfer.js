"use strict";

const assert = require("assert");
const { isAscii, isUtf8, Buffer } = require("buffer");
const { TextEncoder } = require("util");

assert.strictEqual(isAscii(new TextEncoder().encode("hello")), true);

const source = new ArrayBuffer(1);
const view = new Uint8Array(source);
view[0] = 255;
const buffer = Buffer.from(source);
structuredClone(source, { transfer: [source] });

for (const input of [source, view, buffer]) {
  assert.strictEqual(isAscii(input), true);
  assert.strictEqual(isUtf8(input), true);
}
