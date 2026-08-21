const assert = require("assert");
const { PassThrough, pipeline } = require("stream");

const output = new PassThrough();
pipeline(
  (function* () {
    throw new Error("generator failed");
  })(),
  output,
  (error) => {
    assert.strictEqual(error.message, "generator failed");
    assert.strictEqual(output.destroyed, true);
    console.log("stream pipeline generator error passed");
  },
);
