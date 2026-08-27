const assert = require("assert");
const { Readable, finished } = require("stream");

const stream = new Readable({ read() {} });
const result = new Promise((resolve) => finished(stream, resolve));
stream.destroy();
result.then((error) => {
  assert.strictEqual(error.code, "ERR_STREAM_PREMATURE_CLOSE");
});
