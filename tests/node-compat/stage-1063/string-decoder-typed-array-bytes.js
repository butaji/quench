const assert = require("assert");
const { StringDecoder } = require("string_decoder");

const decoder = new StringDecoder("utf8");
const input = new Uint16Array([0x4241]);
assert.strictEqual(decoder.write(input), "AB");
