const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
let calls = 0;
socket.once("connect", () => {
  calls++;
  assert.strictEqual(socket.remoteAddress().port, 12345);
});
socket.connect(12345);
queueMicrotask(() => {
  assert.strictEqual(calls, 1);
  socket.close();
});
