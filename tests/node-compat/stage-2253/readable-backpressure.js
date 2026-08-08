const assert = require("assert");
const { Readable, Writable } = require("stream");

let reads = 0;
let writes = 0;
const readable = new Readable({
  highWaterMark: 4,
  read() {
    reads++;
    if (reads > 3) return this.push(null);
    this.push(Buffer.alloc(8));
  }
});
const writable = new Writable({
  highWaterMark: 1,
  write(_chunk, _encoding, callback) {
    writes++;
    setImmediate(callback);
  }
});
readable.pipe(writable).on("finish", () => {
  assert.strictEqual(reads, 4);
  assert.strictEqual(writes, 3);
  console.log("readable backpressure passed");
});
