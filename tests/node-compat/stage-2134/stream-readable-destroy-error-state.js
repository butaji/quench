const assert = require("assert");
const { Readable } = require("stream");

const expected = new Error("kaboom");
const readable = new Readable({ read() {} });
readable._destroy = (_error, callback) => callback(expected);

readable.on("error", (error) => {
  assert.strictEqual(error, expected);
  assert.strictEqual(readable.errored, expected);
  assert.strictEqual(readable._readableState.errored, expected);
});

readable.destroy();
assert.strictEqual(readable.errored, expected);
assert.strictEqual(readable._readableState.errored, expected);

console.log("stream readable destroy error state pass");
