const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
const payload = Buffer.from("payload");
socket.send(payload, 0, payload.length, 41234, "localhost", (error, bytes) => {
  assert.ifError(error);
  assert.strictEqual(bytes, payload.length);
  socket.close();
  console.log("dgram recursive send overload passed");
});
