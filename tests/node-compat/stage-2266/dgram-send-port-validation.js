const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
for (const port of [-1, 0, 65536]) {
  assert.throws(() => socket.send(Buffer.from("x"), port, "127.0.0.1"), {
    code: "ERR_SOCKET_BAD_PORT",
  });
}
console.log("dgram send port validation passed");
