"use strict";
const assert = require("assert");
const { TransformStream } = require("stream/web");

const output = [];
const source = new ReadableStream({
  start(controller) {
    controller.enqueue("x");
    controller.close();
  },
});
const transform = new TransformStream({
  transform(value, controller) {
    controller.enqueue(value + "!");
  },
});
source.pipeThrough(transform);
transform.readable.getReader().read().then(({ value }) => {
  output.push(value);
  assert.deepStrictEqual(output, ["x!"]);
  console.log("web stream pipeline passed");
});
