const assert = require("assert");
const net = require("net");
const socket = new net.Socket();
socket.on("close", () => {
  assert.strictEqual(socket._handle, null);
  socket.setNoDelay();
  socket.setKeepAlive();
  assert.strictEqual(socket.address(), null);
  socket.bufferSize;
  socket.pause();
  socket.resume();
  console.log("net socket after close passed");
});
socket.end();
