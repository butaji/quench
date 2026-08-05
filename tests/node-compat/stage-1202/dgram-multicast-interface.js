const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, () => {
  assert.strictEqual(socket.setMulticastInterface("0.0.0.0"), socket);
  assert.throws(() => socket.setMulticastInterface(1), TypeError);
  socket.close();
});
