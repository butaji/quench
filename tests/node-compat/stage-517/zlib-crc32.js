const assert = require("assert");
const zlib = require("zlib");

assert.strictEqual(zlib.crc32(""), 0);
assert.strictEqual(zlib.crc32("hello"), 0x3610a686);
assert.strictEqual(zlib.crc32(Buffer.from([0, 1, 2, 255])), 1068644388);
assert.strictEqual(zlib.crc32("hello", 1), 191926070);

console.log("zlib crc32 passed");
