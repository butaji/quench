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
