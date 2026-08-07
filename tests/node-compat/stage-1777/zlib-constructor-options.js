"use strict";
const assert = require("assert");
const zlib = require("zlib");

assert.throws(() => new zlib.Deflate({ chunkSize: "test" }), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => new zlib.Deflate({ chunkSize: 0 }), {
  name: "RangeError",
  code: "ERR_OUT_OF_RANGE",
});
assert.throws(() => new zlib.Deflate({ level: -2 }), {
  name: "RangeError",
  code: "ERR_OUT_OF_RANGE",
});
assert.strictEqual(zlib.constants.Z_MAX_CHUNK, Infinity);
assert.throws(() => new zlib.Deflate({ strategy: "test" }), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => new zlib.Deflate({ dictionary: "not a buffer" }), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_TYPE",
});
assert.strictEqual(typeof new zlib.Deflate().params, "function");
assert.throws(() => new zlib.Deflate().params("test"), {
  name: "TypeError",
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => new zlib.Deflate().params(0, -2), {
  name: "RangeError",
  code: "ERR_OUT_OF_RANGE",
});
console.log("zlib constructor options passed");
