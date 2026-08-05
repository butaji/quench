const assert = require("node:assert");
const { StringDecoder } = require("node:string_decoder");

let decoder = new StringDecoder("utf16le");
assert.strictEqual(decoder.write(Buffer.from("3DD8", "hex")), "");
assert.strictEqual(decoder.write(Buffer.from("4D", "hex")), "");
assert.strictEqual(decoder.write(Buffer.from("DC", "hex")), "👍");
assert.strictEqual(decoder.end(), "");

decoder = new StringDecoder("utf16le");
assert.strictEqual(decoder.write(Buffer.alloc(1)), "");
assert.strictEqual(decoder.write(Buffer.alloc(20)), "\0".repeat(10));
assert.strictEqual(decoder.write(Buffer.alloc(48)), "\0".repeat(24));
assert.strictEqual(decoder.end(), "");
console.log("string decoder utf16 passed");
