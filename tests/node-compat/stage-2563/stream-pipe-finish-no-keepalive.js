const assert = require("assert");
const { Readable, Writable } = require("stream");

let ticks = 17;
const source = new Readable({
  objectMode: true,
  read() {
    if (ticks-- > 0) return process.nextTick(() => source.push({}));
    source.push({});
    source.push(null);
  },
});
const sink = new Writable({
  highWaterMark: 0,
  objectMode: true,
  write(_chunk, _encoding, callback) {
    setImmediate(callback);
  },
});
source.on("end", () => console.log("source ended"));
sink.on("finish", () => console.log("sink finished"));
source.pipe(sink);
