const assert = require("assert");
const { Readable, Writable } = require("stream");

let recursiveWrites = 0;
const recursiveWritable = new Writable({
  highWaterMark: 16 * 1024,
  write(chunk, _encoding, callback) {
    recursiveWrites++;
    assert.strictEqual(
      recursiveReadable._readableState.awaitDrainWriters,
      null
    );
    if (chunk.length === 32 * 1024) {
      recursiveReadable.push(Buffer.alloc(34 * 1024));
      process.nextTick(() => {
        assert.strictEqual(
          recursiveReadable._readableState.awaitDrainWriters,
          recursiveWritable
        );
      });
    }
    process.nextTick(callback);
  }
});
const buffers = [Buffer.alloc(32 * 1024), Buffer.alloc(33 * 1024)];
const recursiveReadable = new Readable({
  highWaterMark: 16 * 1024,
  read() {
    while (buffers.length) this.push(buffers.shift());
  }
});
recursiveReadable.pipe(recursiveWritable);

const multiReadable = new Readable({ read() {} });
const multiSizes = [];
const multiWriters = [0, 1, 2].map(
  (expected) =>
    new Writable({
      write(_chunk, _encoding, callback) {
        multiSizes.push(multiReadable._readableState.awaitDrainWriters.size);
        assert.strictEqual(
          multiReadable._readableState.awaitDrainWriters.size,
          expected
        );
        if (expected === 0) process.nextTick(callback);
      }
    })
);
for (const writer of multiWriters) multiReadable.pipe(writer);
multiReadable.push(Buffer.alloc(560000));

process.on("beforeExit", () => {
  assert.strictEqual(recursiveWrites, 3);
  assert.deepStrictEqual(multiSizes, [0, 1, 2]);
});
