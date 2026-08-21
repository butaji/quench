const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, "localhost", () => {
  assert.strictEqual(socket.address().address, "127.0.0.1");
  socket.close();
  console.log("dgram localhost bind passed");
});
