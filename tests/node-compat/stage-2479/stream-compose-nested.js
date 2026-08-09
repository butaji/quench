const assert = require("assert");
const { compose } = require("stream");

let output = "";
const source = compose(
  (async function* () {
    yield "hello";
    yield "world";
  })()
);
const transform = compose(async function* (values) {
  for await (const value of values) yield value.toUpperCase();
});
const sink = compose(async function (values) {
  for await (const value of values) output += value;
});

const nested = compose(source, transform, sink);
assert.strictEqual(nested.writable, false);
assert.strictEqual(nested.readable, false);

let finished = false;
const keepAlive = setInterval(() => {}, 10);
nested.on("finish", () => {
  assert.strictEqual(output, "HELLOWORLD");
  finished = true;
  clearInterval(keepAlive);
});

process.on("beforeExit", () => {
  assert.strictEqual(finished, true);
});
