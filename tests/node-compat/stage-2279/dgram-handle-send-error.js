const assert = require("assert");
const dgram = require("dgram");
const { kStateSymbol } = require("internal/dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, () => {
  socket[kStateSymbol].handle.send = () => 1;
  socket.send("x", socket.address().port, "127.0.0.1", (error) => {
    assert.strictEqual(error.code, "UNKNOWN");
    socket.close();
    console.log("dgram handle send error passed");
  });
});
