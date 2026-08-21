const assert = require("assert");
const { PassThrough } = require("stream");

const stream = new PassThrough();
assert.strictEqual(stream.readableEnded, false);
assert.strictEqual(stream.writableEnded, false);
assert.strictEqual(stream.read(), null);
stream.end("value");

setImmediate(() => {
  assert.strictEqual(stream.readableEnded, false);
  assert.strictEqual(stream.writableEnded, true);
  assert.strictEqual(stream.read().toString(), "value");
  setImmediate(() => assert.strictEqual(stream.readableEnded, true));
});
