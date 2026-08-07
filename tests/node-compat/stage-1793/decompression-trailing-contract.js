"use strict";
const assert = require("assert");
const { DecompressionStream } = require("stream/web");

const valid = new Uint8Array([120, 156, 75, 4, 0, 0, 98, 0, 98]);
const invalid = new Uint8Array([...valid, 0]);
const output = new Blob([invalid]).stream().pipeThrough(
  new DecompressionStream("deflate"),
);
assert.rejects(Array.fromAsync(output), { name: "TypeError" }).then(() => {
  console.log("decompression trailing contract passed");
});
