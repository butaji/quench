const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
const message = Buffer.from("Some bytes");
socket.send(message, 0, message.length, 41234, "localhost", (error, bytes) => {
  assert.ifError(error);
  assert.strictEqual(bytes, message.length);
  assert.strictEqual(socket.address().address, "0.0.0.0");
  assert.ok(socket.address().port > 0);
  socket.close();
  console.log("dgram implicit send bind passed");
});
