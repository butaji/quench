const assert = require("assert");
const { Readable, Writable } = require("stream");

const source = new Readable({
  read() {
    this.push(null);
  },
});
const destination = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
destination.on("pipe", (received) => {
  assert.strictEqual(received, source);
  console.log("stream pipe event passed");
});
source.pipe(destination);
