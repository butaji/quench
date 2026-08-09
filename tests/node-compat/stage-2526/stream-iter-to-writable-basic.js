const assert = require("assert");
const { push, text, toWritable } = require("stream/iter");

(async () => {
  const { writer, readable } = push({ backpressure: "unbounded" });
  const writable = toWritable(writer);
  writable.write("hello");
  writable.end(" world");
  assert.strictEqual(await text(readable), "hello world");
})();
