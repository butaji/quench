const assert = require("assert");
const { StringDecoder } = require("string_decoder");

const decoder = new StringDecoder("utf8");
assert.strictEqual(decoder.text(Buffer.from([0x41]), 2), "");
