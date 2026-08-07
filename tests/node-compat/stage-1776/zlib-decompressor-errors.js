"use strict";
const assert = require("assert");
const zlib = require("zlib");

const constructors = [
  ["Unzip", () => zlib.Unzip()],
  ["Gunzip", () => zlib.Gunzip()],
  ["Inflate", () => zlib.Inflate()],
  ["InflateRaw", () => zlib.InflateRaw()],
  ["BrotliDecompress", () => zlib.BrotliDecompress()],
  ["ZstdDecompress", () => new zlib.ZstdDecompress()],
];
const seen = new Map();
for (const [name, create] of constructors) {
  const stream = create();
  seen.set(name, 0);
  stream.on("error", () => seen.set(name, seen.get(name) + 1));
  stream.write("this is not valid compressed data.");
}
queueMicrotask(() => {
  for (const [name, count] of seen) assert.strictEqual(count, 1, name);
  console.log("zlib decompressor errors passed");
});
