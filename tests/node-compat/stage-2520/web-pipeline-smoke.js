const assert = require("assert");
const { pipeline } = require("stream");
const {
  ReadableStream,
  WritableStream,
  TransformStream,
} = require("stream/web");
assert.strictEqual(typeof pipeline, "function");
const values = [];
const source = new ReadableStream({
  start(controller) {
    controller.enqueue("x");
    controller.close();
  },
});
const target = new WritableStream({
  write(value) {
    values.push(value);
  },
});
const transform = new TransformStream({
  transform(value, controller) {
    controller.enqueue(value.toUpperCase());
  },
});
pipeline(source, transform, target, (error) => {
  assert.ifError(error);
  assert.deepStrictEqual(values, ["X"]);
});
