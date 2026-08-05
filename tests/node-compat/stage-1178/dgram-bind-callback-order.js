const assert = require("assert");
const dgram = require("dgram");

let initialized = false;
const socket = dgram.createSocket("udp4");
socket.bind(0, () => {
  assert.strictEqual(initialized, true);
  socket.close();
});
initialized = true;
