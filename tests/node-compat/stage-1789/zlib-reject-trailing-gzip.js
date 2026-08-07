"use strict";
const assert = require("assert");
const zlib = require("zlib");

const valid = zlib.gzipSync(Buffer.from("a"));
const double = Buffer.concat([valid, valid]);
assert.deepStrictEqual(zlib.gunzipSync(double), Buffer.from("aa"));
assert.throws(() => zlib.gunzipSync(double, { rejectGarbageAfterEnd: true }), {
  name: "TypeError",
  code: "ERR_TRAILING_JUNK_AFTER_STREAM_END",
});
console.log("zlib reject trailing gzip passed");
