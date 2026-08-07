const assert = require("assert");
const { Duplex, duplexPair } = require("stream");

const [left, right] = duplexPair();
assert(left instanceof Duplex);
assert(right instanceof Duplex);
assert.notStrictEqual(left, right);
const received = [];
left.on("data", (value) => received.push(String(value)));
right.write("foo");
right.end("bar");
setImmediate(() => {
  assert.deepStrictEqual(received, ["foo", "bar"]);
  console.log("stream duplexPair passed");
});
