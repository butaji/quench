const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.connect(12345, () => socket.close());
assert.throws(() => socket.disconnect(), {
  code: "ERR_SOCKET_DGRAM_NOT_CONNECTED",
});
