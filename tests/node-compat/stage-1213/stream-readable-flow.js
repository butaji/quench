const assert = require("assert");
const { Readable, finished } = require("stream");

const readable = new Readable({ read() {} });
assert.strictEqual(readable.resume(), readable);
assert.strictEqual(readable.pause(), readable);

let closeError;
const closed = new Readable({ read() {} });
finished(closed, (error) => (closeError = error));
closed.destroy();
setImmediate(() => {
  assert.strictEqual(closeError.code, "ERR_STREAM_PREMATURE_CLOSE");
});
