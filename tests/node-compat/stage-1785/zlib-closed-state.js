"use strict";
const assert = require("assert");
const zlib = require("zlib");

const decompress = zlib.createGunzip();
assert.strictEqual(decompress._closed, false);
decompress.on("error", () => {
  assert.strictEqual(decompress._closed, true);
  decompress.close();
});
decompress.write("something invalid");
