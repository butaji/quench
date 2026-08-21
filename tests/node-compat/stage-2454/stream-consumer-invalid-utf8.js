const assert = require("assert");
const { Readable } = require("stream");
const { text } = require("stream/consumers");

const bytes = new Uint8Array([0x66, 0x6f, 0x6f, 0xed, 0xa0, 0x80]);
const expected = "foo\ufffd\ufffd\ufffd";
assert.strictEqual(Buffer.from(bytes).toString(), expected);

const readable = new Readable({ read() {} });
let consumed;
text(readable).then((value) => {
  consumed = value;
});
readable.push(bytes);
readable.push(null);

process.on("beforeExit", () => {
  assert.strictEqual(consumed, expected);
});
