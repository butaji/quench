const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, "127.0.0.1", () => {
  assert.deepStrictEqual(socket.address(), {
    address: "127.0.0.1",
    family: "IPv4",
    port: socket.address().port,
  });
  socket.close();
});

assert.throws(() => socket.address(), {
  code: "EBADF",
  message: "getsockname EBADF",
});
