"use strict";

const assert = require("assert");
const a = Buffer.from("hello world");
const b = Buffer.from("hello world");

assert.strictEqual(a.buffer, b.buffer);
assert.notStrictEqual(a.byteOffset, b.byteOffset);
