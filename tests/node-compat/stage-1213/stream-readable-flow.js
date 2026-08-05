const assert = require("assert");
const { Readable, finished } = require("stream");

const readable = new Readable({ read() {} });
assert.strictEqual(readable.resume(), readable);
assert.strictEqual(readable.pause(), readable);

let closeError;
const closed = new Readable({ read() {} });
closed.once = (event, listener) => {
  if (event === "close") closed.closeListener = listener;
};
finished(closed, (error) => (closeError = error));
closed.closeListener();
assert.strictEqual(closeError.code, "ERR_STREAM_PREMATURE_CLOSE");
