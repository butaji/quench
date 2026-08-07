"use strict";
const assert = require("assert");
const zlib = require("zlib");

assert.ok(Object.isFrozen(zlib.codes));
assert.throws(() => {
  zlib.codes.Z_OK = 1;
}, TypeError);
assert.throws(() => {
  zlib.codes = {};
}, TypeError);
assert.strictEqual(zlib.codes.Z_OK, 0);
console.log("zlib immutable codes passed");
