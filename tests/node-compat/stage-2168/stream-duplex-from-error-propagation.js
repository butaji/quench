const assert = require("assert");
const { Duplex, Readable, Writable } = require("stream");

const expected = new Error("readable error");
const readable = new Readable({ read() {} });
const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  }
});
const duplex = Duplex.from({ readable, writable });
duplex.on("error", (error) => assert.strictEqual(error, expected));
writable.on("error", (error) => {
  assert.strictEqual(error, expected);
  console.log("stream duplex error propagation pass");
});
readable.destroy(expected);
