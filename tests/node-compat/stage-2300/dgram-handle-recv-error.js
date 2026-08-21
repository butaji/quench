const assert = require("assert");
const dgram = require("dgram");
const { kStateSymbol } = require("internal/dgram");

const socket = dgram.createSocket("udp4");
const { handle } = socket[kStateSymbol];
socket.on("error", (error) => {
  assert.strictEqual(error.syscall, "recvmsg");
  socket.close();
  console.log("dgram handle recv error passed");
});
socket.bind(() => handle.onmessage(-1, handle, null, null));
