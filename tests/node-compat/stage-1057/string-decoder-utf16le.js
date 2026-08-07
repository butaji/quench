const assert = require("assert");
const { StringDecoder } = require("string_decoder");

const decoder = new StringDecoder("ucs2");
assert.strictEqual(
  decoder.write(Buffer.from("61006200610062006300", "hex")),
  "ababc",
);
