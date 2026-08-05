const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
assert.throws(() => socket.addMembership(), { code: "ERR_MISSING_ARGS" });
socket.bind(0, () => {
  assert.doesNotThrow(() => socket.addMembership("224.0.0.114"));
  assert.doesNotThrow(() => socket.dropMembership("224.0.0.114"));
  socket.close();
});
