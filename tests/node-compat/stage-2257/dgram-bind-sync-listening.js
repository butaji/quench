const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
const address = socket.bindSync({ address: "127.0.0.1", port: 0 });
assert.ok(address.port > 0);
socket.on("listening", () => {
  assert.deepStrictEqual(socket.address(), address);
  socket.close();
  console.log("dgram bindSync listening passed");
});
