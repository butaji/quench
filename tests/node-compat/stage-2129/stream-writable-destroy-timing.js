const assert = require("assert");
const { Writable } = require("stream");

const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  }
});
let closed = false;
writable._destroy = (_error, callback) => callback();
writable.on("close", () => {
  closed = true;
});
writable.destroy();
assert.strictEqual(closed, false);

console.log("stream writable destroy timing pass");
