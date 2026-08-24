"use strict";

const assert = require("assert");
require("../../node/test/common");
const { isAscii, isUtf8, Buffer } = require("buffer");
const { TextEncoder } = require("util");

assert.strictEqual(isAscii(new TextEncoder().encode("hello")), true);
assert.strictEqual(isAscii(new TextEncoder().encode("ğ")), false);
for (const input of [
  undefined,
  "",
  "hello",
  false,
  true,
  0,
  1,
  0n,
  1n,
  Symbol(),
  () => {},
  {},
  [],
  null,
]) {
  assert.throws(() => isAscii(input), { code: "ERR_INVALID_ARG_TYPE" });
}

const source = new ArrayBuffer(1);
const view = new Uint8Array(source);
view[0] = 255;
const buffer = Buffer.from(source);
structuredClone(source, { transfer: [source] });

for (const input of [source, view, buffer]) {
  assert.strictEqual(isAscii(input), true);
  assert.strictEqual(isUtf8(input), true);
}
