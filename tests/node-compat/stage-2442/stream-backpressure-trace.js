const { Readable, Writable } = require("stream");
const events = [];
let reads = 0;
const readable = new Readable({
  highWaterMark: 16,
  read() {
    events.push(`read:${++reads}:len=${this._readableState.length}`);
    if (reads > 3) return this.push(null);
    this.push(Buffer.alloc(32));
    this.push(Buffer.alloc(8));
  }
});
const writable = new Writable({
  highWaterMark: 16,
  write(chunk, _encoding, callback) {
    events.push(`write:${chunk.length}`);
    setImmediate(() => {
      events.push("callback");
      callback();
    });
  }
});
for (const name of ["pause", "resume", "drain", "end"]) {
  readable.on(name, () => events.push(`readable:${name}`));
  writable.on(name, () => events.push(`writable:${name}`));
}
readable.pipe(writable);
setTimeout(() => {
  console.log(JSON.stringify({ reads, events }));
}, 50);
