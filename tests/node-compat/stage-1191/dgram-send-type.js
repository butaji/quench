const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.throws(() => socket.send(true, 0, 1, 1, "127.0.0.1"), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
assert.throws(() => socket.sendto(5, 0, 1, 1, "127.0.0.1"), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
socket.close();
