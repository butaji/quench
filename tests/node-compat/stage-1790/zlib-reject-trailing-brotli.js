"use strict";
const assert = require("assert");
const zlib = require("zlib");

const valid = zlib.brotliCompressSync(Buffer.from("a"));
const double = Buffer.concat([valid, valid]);
assert.deepStrictEqual(zlib.brotliDecompressSync(double), Buffer.from("a"));
assert.throws(
  () => zlib.brotliDecompressSync(double, { rejectGarbageAfterEnd: true }),
  {
    name: "TypeError",
    code: "ERR_TRAILING_JUNK_AFTER_STREAM_END",
  },
);
console.log("zlib reject trailing brotli passed");
