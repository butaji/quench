const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.on("error", (error) => {
  assert.strictEqual(error.code, "EADDRNOTAVAIL");
  assert.strictEqual(error.address, "1.1.1.1");
  assert.strictEqual(error.message, "bind EADDRNOTAVAIL 1.1.1.1");
  socket.close();
  console.log("dgram bind address error passed");
});
socket.bind(0, "1.1.1.1");
