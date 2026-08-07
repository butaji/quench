const assert = require("assert");
const dgram = require("dgram");

assert.throws(
  () => dgram.createSocket({ type: "udp4", recvBufferSize: "invalid" }),
  {
    code: "ERR_INVALID_ARG_TYPE",
  },
);
const socket = dgram.createSocket({
  type: "udp4",
  recvBufferSize: 10000,
  sendBufferSize: 15000,
});
assert.strictEqual(socket.getRecvBufferSize(), 10000);
assert.strictEqual(socket.getSendBufferSize(), 15000);
socket.close();
