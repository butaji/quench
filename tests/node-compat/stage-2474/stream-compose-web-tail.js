const assert = require("assert");
const { compose, Readable } = require("stream");

const source = Readable.from(["hello", " ", "world"]);
const composed = compose(source, new TransformStream());
let output = "";
let ended = false;
const keepAlive = setInterval(() => {}, 10);

composed.on("data", (chunk) => {
  output += Buffer.from(chunk).toString();
});
composed.on("end", () => {
  assert.strictEqual(output, "hello world");
  ended = true;
  clearInterval(keepAlive);
});

process.on("beforeExit", () => {
  assert.strictEqual(ended, true);
});
