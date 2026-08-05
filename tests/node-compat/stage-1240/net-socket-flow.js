const assert = require("assert");
const net = require("net");

const socket = net.createConnection({ port: 0 });
assert.strictEqual(socket.resume(), socket);
assert.strictEqual(socket.pause(), socket);
