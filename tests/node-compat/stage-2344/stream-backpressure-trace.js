const { Readable, Writable } = require("stream");

let reads = 0;
let writes = 0;
const readable = new Readable({
  highWaterMark: 16 * 1024,
  read() {
    reads++;
    console.log(
      "read",
      reads,
      "paused",
      this.isPaused(),
      "buffer",
      this.readableLength
    );
    if (reads === 3) return this.push(null);
    this.push(Buffer.alloc(65500));
    for (let i = 0; i < 40; i++) this.push(Buffer.alloc(1024));
  }
});
const writable = new Writable({
  highWaterMark: 16 * 1024,
  write(chunk, _encoding, callback) {
    writes++;
    console.log("write", writes, chunk.length, "length", this.writableLength);
    setImmediate(() => {
      callback();
      console.log("callback", writes, "needDrain", this.writableNeedDrain);
    });
  }
});
readable.on("pause", () => console.log("pause"));
readable.on("resume", () => console.log("resume"));
writable.on("drain", () => console.log("drain"));
writable.on("finish", () => {
  console.log("finish", reads, writes);
});
readable.pipe(writable);
