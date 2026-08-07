const assert = require("assert");
const { Writable, finished } = require("stream");

const stream = new Writable();
const error = new Error("destroyed");
stream.destroy(error);
finished(stream, (received) => {
  assert.strictEqual(received, error);
});
