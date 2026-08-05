const assert = require("assert");
const net = require("net");

const socket = net.createConnection({ port: 0 });
assert.strictEqual(socket.readable, true);
assert.strictEqual(socket.writable, true);
assert.strictEqual(socket.setEncoding("utf8"), socket);
assert.strictEqual(socket.write("hello"), true);
socket.end();
