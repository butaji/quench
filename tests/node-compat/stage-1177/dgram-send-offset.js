const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
const message = Buffer.allocUnsafe(256);
socket.bind(0, () => {
  const offset = 20;
  const length = message.length - offset;
  socket.send(
    message,
    offset,
    length,
    socket.address().port,
    "127.0.0.1",
    (error, bytes) => {
      assert.strictEqual(error, null);
      assert.strictEqual(bytes, length);
      socket.close();
    },
  );
});
