"use strict";
const assert = require("assert");
const zlib = require("zlib");

assert.throws(() => zlib.createGzip({ flush: "foobar" }), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => zlib.createGzip({ flush: 10000 }), {
  name: "RangeError",
  code: "ERR_OUT_OF_RANGE",
});
assert.throws(() => zlib.createGzip({ finishFlush: "foobar" }), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("zlib flush options passed");
