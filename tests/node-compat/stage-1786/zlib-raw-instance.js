"use strict";
const assert = require("assert");
const zlib = require("zlib");

assert.ok(zlib.createInflateRaw() instanceof zlib.InflateRaw);
assert.ok(zlib.createDeflateRaw() instanceof zlib.DeflateRaw);
console.log("zlib raw instances passed");
