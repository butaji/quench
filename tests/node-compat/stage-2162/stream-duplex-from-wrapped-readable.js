const assert = require("assert");
const { Duplex, Readable } = require("stream");

const msg = Buffer.from("hello");
const readable = Readable({
  read() {
    this.push(msg);
    this.push(null);
  }
});
assert.strictEqual(typeof readable.on, "function");
const duplex = Duplex.from({ readable });
duplex.on("data", (data) => {
  assert.strictEqual(data, msg);
  console.log("stream duplex wrapped readable pass");
});
