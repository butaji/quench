const assert = require("assert");
const net = require("net");

const source = new net.Socket();
const destination = {
  destroyed: false,
  writableEnded: false,
  write() {},
  end() {
    this.writableEnded = true;
  },
};

assert.strictEqual(source.pipe(destination), destination);
source.emit("data", "payload");
source.emit("end");
assert.strictEqual(destination.writableEnded, true);
