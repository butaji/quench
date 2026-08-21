const assert = require("assert");
const net = require("net");

const calls = [];
const socket = new net.Socket({
  handle: { setKeepAlive: (enabled, delay) => calls.push([enabled, delay]) },
});
assert.strictEqual(socket.readyState, "open");
assert.strictEqual(socket.setKeepAlive(true, 3000), socket);
assert.strictEqual(socket.setKeepAlive(false, 1000), socket);
assert.deepStrictEqual(calls, [
  [true, 3],
  [false, 1],
]);
socket.destroy();
assert.strictEqual(socket.readyState, "closed");
console.log("socket keepalive passed");
