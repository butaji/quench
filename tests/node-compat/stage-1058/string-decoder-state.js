const assert = require("assert");
const { StringDecoder } = require("string_decoder");

const decoder = new StringDecoder("utf8");
assert.strictEqual(decoder.write(Buffer.from("E1", "hex")), "");
assert(decoder.lastChar.equals(Buffer.from([0xe1, 0, 0, 0])));
assert.strictEqual(decoder.lastNeed, 2);
assert.strictEqual(decoder.lastTotal, 3);
assert.strictEqual(decoder.end(), "\ufffd");
