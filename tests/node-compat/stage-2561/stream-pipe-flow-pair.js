const assert = require("assert");
const { PassThrough, Readable, Writable } = require("stream");

let ticks = 17;
const first = new Readable({
  objectMode: true,
  read() {
    if (ticks-- > 0) return process.nextTick(() => first.push({}));
    first.push({});
    first.push(null);
  },
});
const sink = new Writable({
  highWaterMark: 0,
  objectMode: true,
  write(_value, _encoding, callback) {
    setImmediate(callback);
  },
});
let firstEnd = false;
let firstFinish = false;
first.on("end", () => firstEnd = true);
sink.on("finish", () => firstFinish = true);
first.pipe(sink);

let missing = 8;
const source = new Readable({
  objectMode: true,
  read() {
    if (missing--) this.push({});
    else this.push(null);
  },
});
const pass = source
  .pipe(new PassThrough({ objectMode: true, highWaterMark: 2 }))
  .pipe(new PassThrough({ objectMode: true, highWaterMark: 2 }));
const wrapper = new Readable({
  objectMode: true,
  read() {
    process.nextTick(() => {
      let value = pass.read();
      if (value === null) {
        pass.once("readable", () => {
          value = pass.read();
          if (value !== null) wrapper.push(value);
        });
      } else wrapper.push(value);
    });
  },
});
pass.on("end", () => wrapper.push(null));
wrapper.resume();
setTimeout(() => {
  assert.ok(firstEnd);
  assert.ok(firstFinish);
  console.log("stream pipe flow pair passed");
}, 30);
