const assert = require("assert");

let controller;
const stream = new ReadableStream(
  {
    start(value) {
      controller = value;
    }
  },
  { highWaterMark: 10, size: (value) => value.length }
);
controller.enqueue("abc");
assert.strictEqual(controller.desiredSize, 7);
controller.enqueue("de");
assert.strictEqual(controller.desiredSize, 5);
const reader = stream.getReader();
Promise.all([
  reader.read().then(({ value }) => assert.strictEqual(value, "abc")),
  reader.read().then(({ value }) => assert.strictEqual(value, "de"))
]).then(() => console.log("readable queue size passed"));
