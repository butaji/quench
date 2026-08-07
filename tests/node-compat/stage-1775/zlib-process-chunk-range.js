"use strict";
const assert = require("assert");
const zlib = require("zlib");

const deflate = zlib.createDeflate();
deflate._outOffset = deflate._chunkSize + 1;
assert.throws(() => {
  deflate._processChunk(Buffer.alloc(1), zlib.constants.Z_FINISH);
}, {
  name: "RangeError",
  code: "ERR_OUT_OF_RANGE",
});
deflate.close();
console.log("zlib process chunk range passed");
