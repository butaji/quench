const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, () => {
  const port = socket.address().port;
  socket.on("message", (message) => {
    assert.strictEqual(message.toString(), "x");
    socket.close();
    console.log("dgram offset port passed");
  });
  socket.send(Buffer.from("xyz"), 0, 1, port);
});
