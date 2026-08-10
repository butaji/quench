const assert = require("assert");
const { compose, PassThrough, Transform } = require("stream");

const objects = compose(
  new PassThrough({ writableObjectMode: false, readableObjectMode: false }),
  new Transform({
    writableObjectMode: false,
    readableObjectMode: true,
    transform(chunk, _encoding, callback) {
      callback(null, { value: chunk.toString() });
    },
  }),
);
assert.strictEqual(objects.writableObjectMode, false);
assert.strictEqual(objects.readableObjectMode, true);
objects.write("first");
objects.end("second");

const bytes = compose(
  new PassThrough({ writableObjectMode: true, readableObjectMode: true }),
  new Transform({
    writableObjectMode: true,
    readableObjectMode: false,
    transform(chunk, _encoding, callback) {
      callback(null, chunk.value);
    },
  }),
);
assert.strictEqual(bytes.writableObjectMode, true);
assert.strictEqual(bytes.readableObjectMode, false);
bytes.write({ value: "first" });
bytes.end({ value: "second" });

let completed = false;
Promise.all([objects.toArray(), bytes.toArray()]).then(
  ([objectValues, buffers]) => {
    assert.deepStrictEqual(objectValues, [
      { value: "first" },
      { value: "second" },
    ]);
    assert.deepStrictEqual(buffers, [
      Buffer.from("first"),
      Buffer.from("second"),
    ]);
    completed = true;
  },
);

process.on("beforeExit", () => {
  assert.strictEqual(completed, true);
});
