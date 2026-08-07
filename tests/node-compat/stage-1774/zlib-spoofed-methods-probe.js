const assert = require("assert");
const zlib = require("zlib");
const spoofed = new Uint8Array(1).fill(0x41);
Object.defineProperty(spoofed, "length", { get: () => 5000 });
Object.defineProperty(spoofed, "byteLength", { get: () => 5000 });
for (
  const name of [
    "deflateSync",
    "gzipSync",
    "deflateRawSync",
    "unzipSync",
    "inflateSync",
    "gunzipSync",
    "inflateRawSync",
    "brotliCompressSync",
    "brotliDecompressSync",
    "zstdCompressSync",
    "zstdDecompressSync",
  ]
) {
  assert.throws(() => zlib[name](spoofed), {
    name: "RangeError",
    code: "ERR_OUT_OF_RANGE",
  });
}
console.log("zlib spoofed methods passed");
