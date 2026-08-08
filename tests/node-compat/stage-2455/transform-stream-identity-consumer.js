const assert = require("assert");
const { blob } = require("stream/consumers");
const { TransformStream } = require("stream/web");

const { readable, writable } = new TransformStream();
const writer = writable.getWriter();
let size;
let contents;

blob(readable).then(async (value) => {
  size = value.size;
  contents = Buffer.from(await value.arrayBuffer()).toString();
});

writer.write("hello");
setTimeout(() => {
  writer.write("there");
  writer.close();
}, 10);

process.on("beforeExit", () => {
  assert.deepStrictEqual(
    { size, contents },
    { size: 10, contents: "hellothere" }
  );
});
