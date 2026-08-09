const assert = require("assert");
const { Readable } = require("stream");
const values = [];
const readable = Readable.from([1, 2, 3]);
readable.on("data", (value) => {
  values.push(value);
  if (value === 1) readable.pause();
});
queueMicrotask(() => {
  assert.deepStrictEqual(values, [1]);
  readable.resume();
});
readable.on("end", () => assert.deepStrictEqual(values, [1, 2, 3]));
