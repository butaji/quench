const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, () => {
  socket.on("message", (message) => {
    assert.strictEqual(message.toString(), "ok");
    socket.close();
    console.log("dgram send callback overload passed");
  });
  socket.send("ok", socket.address().port, (error) => assert.ifError(error));
});
