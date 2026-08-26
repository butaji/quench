const assert = require("assert");
const { Readable } = require("stream");

let reads = 0;
let data = 0;
const stream = new Readable({
  read() {
    reads++;
    this.push(null);
  },
});

stream.on("data", () => data++);
stream.pause();
setTimeout(() => {
  stream.once("end", () => {
    assert.strictEqual(reads, 1);
    assert.strictEqual(data, 0);
  });
  stream.resume();
}, 1);
