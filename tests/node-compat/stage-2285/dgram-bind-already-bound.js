const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(() => {
  assert.throws(() => socket.bind(), {
    code: "ERR_SOCKET_ALREADY_BOUND",
  });
  socket.close();
  console.log("dgram already bound passed");
});
