const assert = require("assert");
const dgram = require("dgram");

for (const type of ["test", ["udp4"], 1, {}, true, false, null]) {
  assert.throws(() => dgram.createSocket(type), {
    code: "ERR_SOCKET_BAD_TYPE",
    name: "TypeError",
  });
}
assert.strictEqual(dgram.createSocket("udp4").type, "udp4");
