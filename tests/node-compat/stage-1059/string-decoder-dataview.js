const assert = require("assert");
const { StringDecoder } = require("string_decoder");

const source = Buffer.from("view input\n");
const decoder = new StringDecoder();
const view = new DataView(source.buffer, source.byteOffset, source.byteLength);
assert.strictEqual(decoder.write(view), "view input\n");
