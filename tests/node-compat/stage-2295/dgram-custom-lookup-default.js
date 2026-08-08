const assert = require("assert");
const dgram = require("dgram");

let calls = 0;
const socket = dgram.createSocket({
  type: "udp4",
  lookup(host, family, callback) {
    calls++;
    assert.strictEqual(host, "0.0.0.0");
    assert.strictEqual(family, 4);
    callback(null, "0.0.0.0", 4);
  }
});
socket.bind(() => {
  assert.strictEqual(calls, 1);
  socket.close();
  console.log("dgram default custom lookup passed");
});
