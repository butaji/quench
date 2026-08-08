const assert = require("assert");
const { Duplex } = require("stream");

assert.ok(Object.hasOwn(Duplex.prototype, "writableFinished"));
const duplex = Duplex({
  write(_chunk, _encoding, callback) {
    callback();
  }
});
assert.strictEqual(duplex.writableFinished, false);
duplex.end("data", () => assert.strictEqual(duplex.writableFinished, true));
duplex.on("finish", () => {
  assert.strictEqual(duplex.writableFinished, true);
  console.log("stream duplex writable finished pass");
});
