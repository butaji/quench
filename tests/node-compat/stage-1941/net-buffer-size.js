const assert = require("assert");
const net = require("net");

const socket = new net.Socket();
for (let i = 1; i < 10; i++) {
  socket.write("a");
  assert.strictEqual(socket.bufferSize, i);
}
socket.on("finish", () => assert.strictEqual(socket.bufferSize, 0));
socket.end();
setTimeout(() => console.log("net bufferSize passed"), 0);
