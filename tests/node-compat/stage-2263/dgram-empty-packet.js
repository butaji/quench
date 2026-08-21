const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, () => {
  socket.connect(socket.address().port, () => {
    socket.on("message", (message) => {
      assert.strictEqual(message.length, 0);
      socket.close();
      console.log("dgram empty packet passed");
    });
    socket.send(Buffer.alloc(1), 0, 0, () => {});
  });
});
