"use strict";

const assert = require("assert");
const zlib = require("zlib");

assert.strictEqual(zlib.constants.Z_OK, 0);
assert.strictEqual(zlib.codes.Z_OK, 0);
assert.ok(Object.isFrozen(zlib.constants));
assert.ok(Object.isFrozen(zlib.codes));

console.log("zlib constants passed");
