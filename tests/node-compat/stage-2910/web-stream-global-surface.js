const assert = require("assert");

const web = require("stream/web");
assert.strictEqual(typeof ReadableStream, "function");
assert.strictEqual(web.ReadableStream, ReadableStream);

let controller;
const stream = new ReadableStream({
  start(value) {
    controller = value;
  },
});
controller.enqueue("ok");
assert.strictEqual(controller.desiredSize, 0);
stream.getReader().read().then(({ value }) => {
  assert.strictEqual(value, "ok");
});
