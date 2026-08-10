const assert = require("assert");
const net = require("net");

const calls = [];
const socket = new net.Socket({
  handle: { setNoDelay: (value) => calls.push(value) },
});
assert.strictEqual(socket.setNoDelay(), socket);
socket.setNoDelay(true);
socket.setNoDelay(false);
socket.setNoDelay(0);
socket.setNoDelay(1);
assert.deepStrictEqual(calls, [true, false, true]);
console.log("socket setNoDelay passed");
