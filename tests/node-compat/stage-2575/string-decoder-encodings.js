const assert = require("assert");
const { StringDecoder } = require("string_decoder");

assert.strictEqual(
  new StringDecoder("latin1").write(Buffer.from([0x41, 0xff])),
  "Aÿ",
);
assert.strictEqual(
  new StringDecoder("ascii").write(Buffer.from([0x41, 0xff])),
  "A",
);

const utf16 = new StringDecoder("utf16le");
assert.strictEqual(utf16.write(Buffer.from([0x41, 0x00, 0x42])), "A");
assert.strictEqual(utf16.end(Buffer.from([0x00])), "B");
console.log("string decoder encodings passed");
const utf8 = new StringDecoder("utf8");
assert.strictEqual(utf8.write(Buffer.from([0xf0])), "");
assert.strictEqual(utf8.lastNeed, 3);
assert.strictEqual(utf8.lastTotal, 4);
assert.strictEqual(utf8.write(Buffer.from([0x9f, 0x98])), "");
assert.strictEqual(utf8.lastNeed, 1);
assert.strictEqual(utf8.lastTotal, 4);
assert.strictEqual(utf8.write(Buffer.from([0x80])), "😀");
assert.strictEqual(utf8.lastNeed, 0);
assert.strictEqual(utf8.lastTotal, 0);
