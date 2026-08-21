const assert = require("assert");
const dgram = require("dgram");

let called = false;
const socket = dgram.createSocket({
  type: "udp4",
  lookup(host, family, callback) {
    called = true;
    assert.strictEqual(host, "example.invalid");
    assert.strictEqual(family, 4);
    callback(null, "127.0.0.1", 4);
  },
});
socket.bind(0, "example.invalid", () => {
  assert.strictEqual(called, true);
  socket.close();
  console.log("dgram custom lookup passed");
});
