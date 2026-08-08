const assert = require("assert");
const common = require("../../../tests/node/common");
const { Readable, Writable } = require("stream");

let reads = 0;
let writes = 0;
const readable = new Readable({
  highWaterMark: 4,
  read: common.mustCall(function () {
    reads++;
    if (reads > 3) return this.push(null);
    this.push(Buffer.alloc(8));
  }, 4)
});
const writable = new Writable({
  highWaterMark: 1,
  write: common.mustCall(function (_chunk, _encoding, callback) {
    writes++;
    setImmediate(callback);
  }, 3)
});

writable.on(
  "finish",
  common.mustCall(() => {
    assert.strictEqual(reads, 4);
    assert.strictEqual(writes, 3);
    console.log("readable backpressure mustCall passed");
  })
);
readable.pipe(writable);
