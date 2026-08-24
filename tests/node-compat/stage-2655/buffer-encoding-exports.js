"use strict";

const assert = require("assert");
const { isAscii, isUtf8, Buffer } = require("buffer");

assert.strictEqual(typeof isAscii, "function");
assert.strictEqual(typeof isUtf8, "function");
assert.strictEqual(isAscii(Buffer.from("hello")), true);
assert.strictEqual(isUtf8(Buffer.from("hello")), true);
const bytes = new Uint8Array(new ArrayBuffer(1));
bytes[0] = 255;
assert.strictEqual(isAscii(bytes), false);
assert.throws(() => isAscii(""), { code: "ERR_INVALID_ARG_TYPE" });
