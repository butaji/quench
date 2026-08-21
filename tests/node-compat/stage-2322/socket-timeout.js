const assert = require("assert");
const net = require("net");

const socket = new net.Socket();
let timedOut = false;
socket.once("timeout", () => {
  timedOut = true;
});
assert.strictEqual(socket.setTimeout(1), socket);
setTimeout(() => {
  assert.ok(timedOut);
  socket.setTimeout(0);
  socket.destroy();
  console.log("socket timeout passed");
}, 10);
