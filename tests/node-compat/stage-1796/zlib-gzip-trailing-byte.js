"use strict";
const assert = require("assert");
const zlib = require("zlib");
const valid = zlib.gzipSync(Buffer.from("a"));
const invalid = Buffer.concat([valid, Buffer.from([0])]);
assert.throws(() => zlib.gunzipSync(invalid, { rejectGarbageAfterEnd: true }), {
  name: "TypeError",
  code: "ERR_TRAILING_JUNK_AFTER_STREAM_END",
});
console.log("zlib gzip trailing byte passed");
