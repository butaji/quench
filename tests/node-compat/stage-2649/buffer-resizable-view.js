"use strict";
const assert = require("assert");
const backing = new ArrayBuffer(10, { maxByteLength: 20 });
const view = Buffer.from(backing, 1);
assert.strictEqual(view.byteLength, 9);
backing.resize(15);
assert.strictEqual(view.byteLength, 14);
backing.resize(5);
assert.strictEqual(view.byteLength, 4);
