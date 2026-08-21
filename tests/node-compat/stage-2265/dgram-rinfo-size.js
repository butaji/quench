const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, () => {
  socket.on("message", (message, rinfo) => {
    assert.strictEqual(message.length, 3);
    assert.strictEqual(rinfo.size, 3);
    socket.close();
    console.log("dgram rinfo size passed");
  });
  socket.send("abc", socket.address().port);
});
