"use strict";

const assert = require("assert");
const buffer = Buffer.from("abcdef");

assert.strictEqual(buffer.indexOf("bc"), 1);
assert.strictEqual(buffer.indexOf("bc", 2), -1);
assert.strictEqual(buffer.indexOf(0x64), 3);
assert.strictEqual(buffer.indexOf(new Uint8Array([0x64])), 3);
assert.strictEqual(buffer.indexOf("64", 0, "hex"), 3);
assert.strictEqual(buffer.indexOf("", Infinity), buffer.length);
assert.strictEqual(buffer.lastIndexOf("a"), 0);
assert.strictEqual(buffer.lastIndexOf("bc", 2), 1);
assert.strictEqual(buffer.lastIndexOf(Buffer.from("ef")), 4);
assert.strictEqual(buffer.lastIndexOf("64", Infinity, "hex"), 3);
assert.throws(() => buffer.indexOf({}, 0), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
