const assert = require("node:assert");
const zlib = require("node:zlib");

assert.strictEqual(zlib.crc32("hello"), 0x3610a686);
assert.strictEqual(zlib.crc32(Buffer.from("hello")), 0x3610a686);
assert.strictEqual(zlib.crc32("abacus"), 0xc3d7115b);
assert.strictEqual(zlib.crc32("hello", 0x3610a686), 0xf58c9768);
console.log("zlib crc32 passed");
