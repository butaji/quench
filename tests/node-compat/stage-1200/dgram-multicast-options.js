const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.throws(
  () => socket.setMulticastLoopback(1),
  /^Error: setMulticastLoopback EBADF$/,
);
socket.bind(0, () => {
  assert.strictEqual(socket.setMulticastLoopback(1), 1);
  assert.strictEqual(socket.setMulticastTTL(16), 16);
  assert.throws(
    () => socket.setMulticastTTL(1000),
    /^Error: setMulticastTTL EINVAL$/,
  );
  socket.close();
});
