const assert = require("assert");
const { StringDecoder } = require("string_decoder");

const decoder = new StringDecoder("utf8");
assert.strictEqual(
  decoder.write(Buffer.from("C9B5A941", "hex")),
  "\u0275\ufffdA",
);
