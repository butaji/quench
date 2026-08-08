const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind(0, () => {
  socket.setMulticastInterface("0.0.0.0");
  assert.throws(() => socket.setMulticastInterface("224.0.0.2"), /EINVAL/);
  assert.throws(
    () => socket.setMulticastInterface("239.255.255.255"),
    /EINVAL/
  );
  socket.close(() =>
    console.log("dgram multicast interface validation passed")
  );
});
