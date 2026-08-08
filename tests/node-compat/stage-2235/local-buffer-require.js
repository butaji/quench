const assert = require("assert");
const { Blob } = require("buffer");
const BlobFromLocalModule = require("./local-buffer-module");

assert.strictEqual(new Blob([], { type: false }).type, "false");
assert.strictEqual(new Blob([], { type: {} }).type, "[object object]");
assert.strictEqual(new BlobFromLocalModule([], { type: false }).type, "false");
assert.strictEqual(
  new BlobFromLocalModule([], { type: {} }).type,
  "[object object]"
);
console.log("local buffer Blob normalization passed");
