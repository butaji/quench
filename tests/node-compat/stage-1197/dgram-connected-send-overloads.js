const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.connect(12345, () => {
  assert.doesNotThrow(() => socket.send(Buffer.alloc(8), 0, 8));
  assert.doesNotThrow(() => socket.send(Buffer.alloc(8)));
  socket.close();
});
