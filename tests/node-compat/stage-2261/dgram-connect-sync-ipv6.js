const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp6");
socket.connectSync(12345);
assert.deepStrictEqual(socket.remoteAddress(), {
  address: "::1",
  family: "IPv6",
  port: 12345,
});
assert.ok(socket.address().port > 0);
socket.close();
console.log("dgram connectSync IPv6 passed");
