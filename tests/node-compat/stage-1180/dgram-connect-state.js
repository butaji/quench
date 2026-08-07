const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.connect(12345, () => {
  assert.strictEqual(socket.remoteAddress().port, 12345);
  socket.disconnect();
  assert.throws(() => socket.disconnect(), {
    code: "ERR_SOCKET_DGRAM_NOT_CONNECTED",
  });
  socket.close();
});
assert.throws(() => socket.connect(0), { code: "ERR_SOCKET_BAD_PORT" });
