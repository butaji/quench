const assert = require("assert");
const { Writable } = require("stream");

const first = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  }
});
let firstClosed = false;
first.on("close", () => {
  firstClosed = true;
});
first.destroy();
assert.strictEqual(first.destroyed, true);

const expected = new Error("kaboom");
const second = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  }
});
let secondError;
second.on("error", (error) => {
  secondError = error;
});
second.destroy(expected);
assert.strictEqual(second.destroyed, true);

console.log("stream writable destroy basics pass");
