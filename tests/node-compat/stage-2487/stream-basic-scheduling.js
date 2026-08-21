const assert = require("assert");
const { Readable, Transform } = require("stream");

const transformed = [];
const transform = new Transform({
  transform(chunk, _encoding, callback) {
    callback(null, String(chunk).toUpperCase());
  },
});
transform.on("data", (chunk) => transformed.push(chunk));
transform.end("hello", () => {
  assert.deepStrictEqual(transformed, [Buffer.from("HELLO")]);
});

const values = [];
const readable = Readable.from([1, 2, 3]);
readable.on("data", (value) => {
  values.push(value);
  if (value === 1) readable.pause();
});
readable.on("end", () => assert.deepStrictEqual(values, [1, 2, 3]));
queueMicrotask(() => {
  assert.deepStrictEqual(values, [1]);
  readable.resume();
});
