const zlib = require("zlib");

if (
  zlib.constants.BROTLI_MIN_QUALITY !== 0 ||
  zlib.constants.BROTLI_MAX_QUALITY !== 11 ||
  zlib.constants.BROTLI_PARAM_QUALITY !== 1 ||
  zlib.constants.BROTLI_OPERATION_FINISH !== 2
) {
  throw new Error("Brotli constants do not match Node");
}

console.log("zlib Brotli constants passed");
