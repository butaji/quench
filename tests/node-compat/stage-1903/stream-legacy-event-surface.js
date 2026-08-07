const assert = require("assert");
const { Stream, Readable } = require("stream");

(async () => {
  const stream = new Stream();
  assert.strictEqual(typeof stream.emit, "function");
  stream[Symbol.asyncIterator] = Readable.prototype[Symbol.asyncIterator];
  const values = [];
  process.nextTick(() => {
    stream.emit("data", "hello");
    stream.emit("data", "world");
    stream.emit("end");
  });
  for await (const value of stream) values.push(value);
  assert.deepStrictEqual(values, ["hello", "world"]);
})();
