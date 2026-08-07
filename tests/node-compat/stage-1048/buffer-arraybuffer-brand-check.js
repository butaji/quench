const assert = require("assert");
const { Buffer } = require("buffer");

function FakeArrayBuffer() {}
Object.setPrototypeOf(FakeArrayBuffer, ArrayBuffer);
Object.setPrototypeOf(FakeArrayBuffer.prototype, ArrayBuffer.prototype);

assert.throws(() => Buffer.from(new FakeArrayBuffer()), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});

const backing = new ArrayBuffer(4);
const view = Buffer.from(backing);
assert.strictEqual(view.buffer, backing);
