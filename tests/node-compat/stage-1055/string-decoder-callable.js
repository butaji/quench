const assert = require("assert");
const { StringDecoder } = require("string_decoder");

const decoder = {};
StringDecoder.call(decoder);
assert.strictEqual(decoder.encoding, "utf8");
assert.strictEqual(decoder.end(Buffer.from("ok")), "ok");
