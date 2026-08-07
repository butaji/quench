const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.throws(() => socket.sendto(), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(
  () => socket.sendto("buffer", 1, "offset", "port", "address", "cb"),
  {
    code: "ERR_INVALID_ARG_TYPE",
  },
);
assert.throws(
  () => socket.sendto("buffer", "offset", 1, "port", "address", "cb"),
  {
    code: "ERR_INVALID_ARG_TYPE",
  },
);
assert.throws(() => socket.sendto("buffer", 1, 1, 10, false, "cb"), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => socket.sendto("buffer", 1, 1, false, "address", "cb"), {
  code: "ERR_INVALID_ARG_TYPE",
});
socket.close();
