const assert = require("assert");
const { Readable, PassThrough } = require("stream");

const source = new Readable({ read() {} });
const destination = source.pipe(
  new PassThrough({ objectMode: true, highWaterMark: 2 })
);
let finished = false;

destination.on("finish", () => {
  finished = true;
});

source.push("first");
source.push("second");
source.push(null);

setImmediate(() => {
  assert.strictEqual(destination.readableLength, 2);
  assert.strictEqual(
    finished,
    false,
    `finish emitted with ${destination._readableChunks?.length ?? "unknown"} unread chunks`
  );
});
