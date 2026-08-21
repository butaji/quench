const assert = require("assert");
const { Readable, Writable } = require("stream");
const { pipeline } = require("stream/promises");

const source = new Readable({ read() {} });
const sink = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const failure = pipeline(source, sink);
source.push("data");
source.destroy();

failure.then(
  () => assert.fail("pipeline should reject after premature readable close"),
  (error) => assert.strictEqual(error.code, "ERR_STREAM_PREMATURE_CLOSE"),
);
