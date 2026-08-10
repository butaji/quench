const { Readable, Writable } = require("stream");
let reads = 0;
let writes = 0;
const source = new Readable({
  read() {
    reads++;
    if (reads === 11) return this.push(null);
    this.push(Buffer.alloc(65500));
    for (let i = 0; i < 40; i++) this.push(Buffer.alloc(1024));
  },
});
const target = new Writable({
  write(_chunk, _encoding, callback) {
    writes++;
    setImmediate(callback);
  },
});
source.pipe(target);
target.on("finish", () => {
  console.log(
    JSON.stringify({
      reads,
      writes,
      ended: source.readableEnded,
      finished: target.writableFinished,
    }),
  );
});
