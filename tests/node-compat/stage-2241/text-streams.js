const assert = require("assert");
const { TextEncoderStream, TextDecoderStream } = require("stream/web");

assert.throws(() => new TextDecoderStream("latin1"), {
  code: "ERR_ENCODING_NOT_SUPPORTED",
});
assert.throws(() => new TextDecoderStream("utf-8", 1), {
  code: "ERR_INVALID_ARG_TYPE",
});

const encoder = new TextEncoderStream();
assert.strictEqual(encoder.encoding, "utf-8");
const decoder = new TextDecoderStream();
assert.strictEqual(decoder.encoding, "utf-8");
const writer = encoder.writable.getWriter();
writer.write("hello").then(() => writer.close());
console.log("text streams validation passed");
