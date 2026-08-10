const assert = require("assert");
const webstreams = require("stream/web");

for (
  const name of [
    "ReadableStream",
    "ReadableStreamDefaultReader",
    "ReadableStreamBYOBReader",
    "ReadableStreamBYOBRequest",
    "ReadableByteStreamController",
    "ReadableStreamDefaultController",
    "TransformStream",
    "TransformStreamDefaultController",
    "WritableStream",
    "WritableStreamDefaultWriter",
    "WritableStreamDefaultController",
    "ByteLengthQueuingStrategy",
    "CountQueuingStrategy",
    "TextEncoderStream",
    "TextDecoderStream",
    "CompressionStream",
    "DecompressionStream",
  ]
) {
  if (typeof globalThis[name] === "function") {
    assert.strictEqual(globalThis[name], webstreams[name], name);
  }
}
console.log("web streams module identity passed");
