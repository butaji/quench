const assert = require("assert");
const { Readable, Writable } = require("stream");

const writable = new Writable({ highWaterMark: 5 });
const queued = [];
let buffering = true;
let pauses = 0;
let finished = false;

writable._write = (_chunk, _encoding, callback) => {
  if (buffering) queued.push(callback);
  else callback();
};

const readable = new Readable({ read() {} });
readable.pipe(writable);
readable.on("pause", () => {
  pauses++;
  assert.strictEqual(readable._readableState.awaitDrainWriters, writable);
  if (pauses === 1) {
    process.nextTick(() => readable.resume());
  } else if (pauses === 2) {
    buffering = false;
    for (const callback of queued) callback();
  }
});

readable.push(Buffer.alloc(100));
readable.push(Buffer.alloc(100));
readable.push(Buffer.alloc(100));
readable.push(null);

writable.on("finish", () => {
  finished = true;
  assert.strictEqual(readable._readableState.awaitDrainWriters, null);
});

process.on("beforeExit", () => {
  assert.deepStrictEqual({ pauses, finished }, { pauses: 2, finished: true });
});
