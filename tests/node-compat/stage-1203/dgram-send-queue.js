const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.strictEqual(socket.getSendQueueSize(), 0);
assert.strictEqual(socket.getSendQueueCount(), 0);
socket.connect(12345, () => {
  socket.send("hello");
  socket.send("hello");
  assert.strictEqual(socket.getSendQueueSize(), 10);
  assert.strictEqual(socket.getSendQueueCount(), 2);
  socket.close();
});
