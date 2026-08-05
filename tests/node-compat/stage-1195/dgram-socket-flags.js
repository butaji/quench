const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.throws(() => socket.setBroadcast(true), /^Error: setBroadcast EBADF$/);
assert.throws(() => socket.setTTL(64), /^Error: setTTL EBADF$/);
socket.bind(0, () => {
  assert.strictEqual(socket.setBroadcast(true), true);
  assert.strictEqual(socket.setTTL(64), 64);
  socket.close();
});
