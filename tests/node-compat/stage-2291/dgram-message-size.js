const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.send(Buffer.alloc(65508), 12345, "127.0.0.1", (error) => {
  assert.strictEqual(error.code, "EMSGSIZE");
  assert.strictEqual(error.address, "127.0.0.1");
  assert.strictEqual(error.port, 12345);
  socket.close();
  console.log("dgram message size passed");
});
