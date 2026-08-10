const assert = require("assert");
const { Readable, Writable, pipeline } = require("stream");

const expected = new Error("function stage failed");
const source = Readable.from(["input"]);
const sink = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});

pipeline(
  source,
  async function rejectedStage() {
    throw expected;
  },
  sink,
  (error) => {
    assert.strictEqual(error, expected);
    console.log("stream pipeline rejected function passed");
  },
);
