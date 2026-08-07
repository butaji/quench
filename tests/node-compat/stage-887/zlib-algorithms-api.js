"use strict";

const assert = require("assert");
const zlib = require("node:zlib");

for (
  const name of [
    "deflateRaw",
    "deflateRawSync",
    "inflateRaw",
    "inflateRawSync",
    "brotliCompress",
    "brotliCompressSync",
    "brotliDecompress",
    "brotliDecompressSync",
    "unzip",
    "unzipSync",
  ]
) {
  assert.strictEqual(typeof zlib[name], "function");
}

console.log("zlib algorithms api passed");
