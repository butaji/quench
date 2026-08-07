"use strict";
const assert = require("assert");
const { DecompressionStream } = require("stream/web");

const valid = new Uint8Array([120, 156, 75, 4, 0, 0, 98, 0, 98]);
const source = new ReadableStream({
  start(controller) {
    controller.enqueue(valid);
    controller.enqueue(new Uint8Array([0]));
    controller.close();
  },
});
assert.rejects(
  Array.fromAsync(source.pipeThrough(new DecompressionStream("deflate"))),
  { name: "TypeError" },
).then(() => console.log("decompression multichunk passed"));
