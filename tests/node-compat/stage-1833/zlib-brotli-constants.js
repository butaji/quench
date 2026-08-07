const zlib = require("zlib");

if (zlib.constants.BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING !== 4) {
  throw new Error("Brotli constants are incomplete");
}

let error;
try {
  zlib.createBrotliCompress({
    params: {
      [zlib.constants.BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING]: 42,
    },
  });
} catch (caught) {
  error = caught;
}

if (error?.code !== "ERR_ZLIB_INITIALIZATION_FAILED") {
  throw new Error(`unexpected Brotli option error: ${error?.code}`);
}

console.log("zlib Brotli constants passed");
