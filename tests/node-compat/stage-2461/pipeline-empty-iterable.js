const assert = require("assert");
const { pipeline, PassThrough } = require("stream");

let called = 0;
const destination = pipeline(
  "",
  new PassThrough({ objectMode: true }),
  (error) => {
    assert.strictEqual(error, undefined);
    called++;
  },
);

assert(destination instanceof PassThrough);
process.on("beforeExit", () => assert.strictEqual(called, 1));
