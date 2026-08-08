const assert = require("assert");
const { Readable, Writable } = require("stream");

let remaining = 17;
let reads = 0;
let writes = 0;
let ended = false;
let finished = false;

const source = new Readable({
  objectMode: true,
  read() {
    reads++;
    if (remaining-- > 0) {
      process.nextTick(() => source.push({}));
      return;
    }
    source.push({});
    source.push(null);
  }
});

const destination = new Writable({
  objectMode: true,
  highWaterMark: 0,
  write(_chunk, _encoding, callback) {
    writes++;
    setImmediate(callback);
  }
});

source.on("end", () => {
  ended = true;
});
destination.on("finish", () => {
  finished = true;
});
source.pipe(destination);

process.on("beforeExit", () => {
  const actual = { reads, writes, ended, finished };
  assert.deepStrictEqual(
    actual,
    { reads: 18, writes: 18, ended: true, finished: true },
    JSON.stringify(actual)
  );
});
