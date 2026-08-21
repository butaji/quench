const assert = require("assert");
const { Duplex } = require("stream");

const readable = new ReadableStream({
  start(controller) {
    controller.enqueue("foo");
    controller.close();
  },
});
const readableDuplex = Duplex.from(readable);
assert.strictEqual(readableDuplex.readable, true);
assert.strictEqual(readableDuplex.writable, false);

const writable = new WritableStream({ write() {} });
const writableDuplex = Duplex.from(writable);
assert.strictEqual(writableDuplex.readable, false);
assert.strictEqual(writableDuplex.writable, true);

console.log("stream duplex from web side state pass");
