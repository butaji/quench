const assert = require("assert");
const { PassThrough, Readable } = require("stream");

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
const values = [];
const wrapper = new Readable({
  objectMode: true,
  read() {
    process.nextTick(() => {
      let value = pass.read();
      if (value === null) {
        pass.once("readable", () => {
          value = pass.read();
          if (value !== null) this.push(value);
        });
      } else {
        this.push(value);
      }
    });
  },
});
pass.on("end", () => wrapper.push(null));
wrapper.on("data", (value) => values.push(value));
wrapper.on("end", () => {
  assert.strictEqual(values.length, 9);
  console.log("pass-through readable chain passed");
});
wrapper.resume();
