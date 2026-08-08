const assert = require("assert");
const { Writable } = require("stream");

const expected = new Error("kaboom");
const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  }
});
writable._destroy = (error, callback) => {
  assert.strictEqual(error, expected);
  callback(error);
};
let received;
writable.on("error", (error) => {
  received = error;
});
writable.on("close", () => assert.strictEqual(writable.destroyed, true));
writable.destroy(expected);
assert.strictEqual(writable.errored, expected);

console.log("stream writable custom destroy pass");
