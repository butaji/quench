const assert = require("assert");
const { Readable } = require("stream");

const expected = new Error("kaboom");
const readable = new Readable({ read() {} });
readable._destroy = (error, callback) => {
  assert.strictEqual(error, null);
  callback(expected);
};
let received;
readable.on("error", (error) => {
  received = error;
});
readable.destroy();
assert.strictEqual(readable.errored, expected);

console.log("stream destroy callback error pass");
