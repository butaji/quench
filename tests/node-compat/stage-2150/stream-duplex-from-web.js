const assert = require("assert");
const { Duplex } = require("stream");

const readable = new ReadableStream({
  start(controller) {
    controller.enqueue(Buffer.from("hello"));
  },
});
const writable = new WritableStream({
  write(chunk) {
    assert.deepStrictEqual(chunk, Buffer.from("world"));
  },
});
const duplex = Duplex.fromWeb({ readable, writable });
duplex.write(Buffer.from("world"));
duplex.once("data", (chunk) => {
  assert.deepStrictEqual(chunk, Buffer.from("hello"));
  console.log("stream duplex from web pass");
});
