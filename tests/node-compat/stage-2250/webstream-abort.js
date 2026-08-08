const assert = require("assert");
const { addAbortSignal } = require("stream");

const stream = new ReadableStream({
  start(controller) {
    controller.enqueue("value");
  }
});
const reader = stream.getReader();
const controller = new AbortController();
addAbortSignal(controller.signal, stream);
const first = reader.read();
controller.abort();
Promise.all([
  first.then(({ value }) => assert.strictEqual(value, "value")),
  assert.rejects(reader.closed, { name: "AbortError" })
]).then(() => console.log("Web Stream abort passed"));
