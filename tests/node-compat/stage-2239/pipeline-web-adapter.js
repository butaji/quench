const assert = require("assert");
const { Duplex, pipeline } = require("stream");
const { ReadableStream, WritableStream } = require("stream/web");

let controller;
const values = [];
const readable = new ReadableStream({
  start(value) {
    controller = value;
  }
});
const writable = new WritableStream({
  write(value) {
    values.push(value);
  }
});

pipeline(readable, writable, (error) => {
  assert.ifError(error);
  assert.deepStrictEqual(values, ["one", "two"]);
  console.log("web pipeline adapter passed");
});
controller.enqueue("one");
controller.enqueue("two");
controller.close();
