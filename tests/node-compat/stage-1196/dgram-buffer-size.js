const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.throws(() => socket.getSendBufferSize(), {
  code: "ERR_SOCKET_BUFFER_SIZE",
});
socket.bind(0, () => {
  socket.setRecvBufferSize(10000);
  socket.setSendBufferSize(10000);
  assert.strictEqual(socket.getRecvBufferSize(), 20000);
  assert.strictEqual(socket.getSendBufferSize(), 20000);
  socket.close();
});
