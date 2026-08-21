const assert = require("assert");
const { Readable, PassThrough } = require("stream");

let missing = 8;
let forwarded = 0;
let ended = false;

const source = new Readable({
  objectMode: true,
  read() {
    if (missing--) this.push({});
    else this.push(null);
  },
});

const through = source
  .pipe(new PassThrough({ objectMode: true, highWaterMark: 2 }))
  .pipe(new PassThrough({ objectMode: true, highWaterMark: 2 }));

through.on("end", () => wrapper.push(null));

const wrapper = new Readable({
  objectMode: true,
  read() {
    process.nextTick(() => {
      let data = through.read();
      if (data === null) {
        through.once("readable", () => {
          data = through.read();
          if (data !== null) {
            forwarded++;
            wrapper.push(data);
          }
        });
      } else {
        forwarded++;
        wrapper.push(data);
      }
    });
  },
});

wrapper.on("end", () => {
  ended = true;
});
wrapper.resume();

process.on("beforeExit", () => {
  const actual = { forwarded, ended };
  assert.deepStrictEqual(
    actual,
    { forwarded: 8, ended: true },
    JSON.stringify(actual),
  );
});
