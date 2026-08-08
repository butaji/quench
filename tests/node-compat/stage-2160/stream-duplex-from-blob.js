const assert = require("assert");
const { Blob } = require("buffer");
const { Duplex } = require("stream");

const duplex = Duplex.from(new Blob(["blob"]));
assert.strictEqual(duplex.readable, true);
duplex.once("data", (chunk) => {
  assert.strictEqual(Buffer.from(chunk).toString(), "blob");
  console.log("stream duplex from blob pass");
});
