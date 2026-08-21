const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.connect(12345, () => {
  assert.throws(
    () => socket.send(Buffer.from("x"), 1234, "127.0.0.1", () => {}),
    { code: "ERR_SOCKET_DGRAM_IS_CONNECTED", message: "Already connected" },
  );
  socket.send(Buffer.from("hello"), 0, 5, () => {});
  socket.close();
  console.log("dgram connected port rejection passed");
});
