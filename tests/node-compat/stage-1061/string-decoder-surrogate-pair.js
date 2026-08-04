const assert = require("assert");
const { StringDecoder } = require("string_decoder");

const decoder = new StringDecoder("utf16le");
assert.strictEqual(decoder.write(Buffer.from("3DD8", "hex")), "");
assert.strictEqual(decoder.write(Buffer.from("4D", "hex")), "");
assert.strictEqual(decoder.write(Buffer.from("DC", "hex")), "\ud83d\udc4d");
