"use strict";

const assert = require("assert");
const { Buffer } = require("buffer");

assert.deepStrictEqual([...Buffer([1, 2, 3])], [1, 2, 3]);
assert.deepStrictEqual([...new Buffer([4, 5, 6])], [4, 5, 6]);
assert.strictEqual(Buffer.alloc(2).length, 2);

console.log("ok");
