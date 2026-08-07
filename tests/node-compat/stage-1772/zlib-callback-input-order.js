"use strict";
const assert = require("assert");
const zlib = require("zlib");

assert.throws(() => zlib.gunzip(1), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => zlib.gunzip(undefined), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("zlib callback input order passed");
