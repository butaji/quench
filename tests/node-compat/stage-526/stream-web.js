"use strict";

const assert = require("assert");
const { ReadableStream, WritableStream } = require("stream/web");

(async () => {
  const readable = new ReadableStream({
    start(controller) {
      controller.enqueue("web");
      controller.close();
    },
  });
  const reader = readable.getReader();
  assert.deepStrictEqual(await reader.read(), { value: "web", done: false });
  assert.deepStrictEqual(await reader.read(), { value: undefined, done: true });
  const values = [];
  const writer = new WritableStream({
    write(value) {
      values.push(value);
    },
  }).getWriter();
  await writer.write("streams");
  await writer.close();
  assert.deepStrictEqual(values, ["streams"]);
  console.log("stream web passed");
})();
