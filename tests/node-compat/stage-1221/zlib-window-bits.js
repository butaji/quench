const assert = require("assert");
const zlib = require("zlib");

assert.throws(() => zlib.gzipSync(Buffer.alloc(0), { windowBits: 8 }), {
  code: "ERR_OUT_OF_RANGE",
});
