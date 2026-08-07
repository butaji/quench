const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.throws(() => socket.send(), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
socket.close();
