const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
const payload = Buffer.alloc(16);
socket.bind(0, () => {
  socket.send(payload, 4, 12, socket.address().port, (error, bytes) => {
    assert.strictEqual(error, null);
    assert.strictEqual(bytes, 12);
    socket.close();
  });
});
