const assert = require("assert");
const { Readable, Writable } = require("stream");

let ticks = 17;
const readable = new Readable({
  objectMode: true,
  read() {
    if (ticks-- > 0) return process.nextTick(() => readable.push({}));
    readable.push({});
    readable.push(null);
  },
});
const writable = new Writable({
  highWaterMark: 0,
  objectMode: true,
  write(_chunk, _encoding, callback) {
    setImmediate(callback);
  },
});
let readableEnded = false;
let writableFinished = false;
readable.on("end", () => readableEnded = true);
writable.on("finish", () => writableFinished = true);
readable.pipe(writable);
setTimeout(() => {
  assert.ok(readableEnded, "readable did not end");
  assert.ok(writableFinished, "writable did not finish");
  console.log("stream pipe finish trace passed");
}, 20);
