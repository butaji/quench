const assert = require("assert");
const { Readable } = require("stream");

let reads = 0;
const readable = new Readable({
  read() {
    reads++;
    this.push(null);
  },
});
assert.strictEqual(readable._readableState.reading, false);
readable.on("data", () => {});
Promise.resolve().then(() => {
  assert.strictEqual(reads, 1);
  assert.strictEqual(readable._readableState.reading, false);
  assert.strictEqual(readable._readableState.ended, true);
});
