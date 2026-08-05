const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
const first = Buffer.from("x");
const second = Buffer.from("y");
socket.bind(0, () => {
  socket.connect(socket.address().port, "127.0.0.1", () => {
    socket.send([first, second], (error, bytes) => {
      assert.strictEqual(error, null);
      assert.strictEqual(bytes, 2);
      socket.close();
    });
  });
});
