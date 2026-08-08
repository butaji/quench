const assert = require("assert");
const net = require("net");

const socket = new net.Socket();
assert.strictEqual(socket.hasRef(), true);
assert.strictEqual(socket.unref(), socket);
assert.strictEqual(socket.hasRef(), false);
assert.strictEqual(socket.ref(), socket);
assert.strictEqual(socket.hasRef(), true);
console.log("socket ref state passed");
