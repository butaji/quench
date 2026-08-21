const assert = require("assert");
const dgram = require("dgram");
const { kStateSymbol } = require("internal/dgram");

const socket = dgram.createSocket("udp4");
socket.bind(() => {
  const other = dgram.createSocket("udp4");
  assert.throws(() => other.bind({ fd: socket[kStateSymbol].handle.fd }), {
    code: "EEXIST",
  });
  socket.close();
  other.close();
  console.log("dgram bind fd errors passed");
});
