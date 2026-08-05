const assert = require("assert");
const net = require("net");

const socket = net.createConnection({ port: 0 });
assert.strictEqual(socket.resetAndDestroy(), socket);
assert.strictEqual(socket.destroyed, true);
assert.strictEqual(socket.destroy(), socket);
