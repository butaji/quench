"use strict";

const assert = require("assert");
const zlib = require("node:zlib");

for (
  const name of [
    "deflate",
    "deflateSync",
    "inflate",
    "inflateSync",
    "gzip",
    "gzipSync",
    "gunzip",
    "gunzipSync",
    "createDeflate",
    "createInflate",
    "createGzip",
    "createGunzip",
  ]
) {
  assert.strictEqual(typeof zlib[name], "function");
}
assert.strictEqual(typeof zlib.constants, "object");
const compressed = zlib.gzipSync("quench");
assert.ok(compressed);
assert.strictEqual(String(zlib.gunzipSync(compressed)), "quench");

console.log("zlib api passed");
