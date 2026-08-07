const assert = require("assert");
const net = require("net");

const server = net.createServer();
assert.strictEqual(server.listening, false);
assert.strictEqual(server.connections, undefined);
assert.strictEqual(server.maxConnections, undefined);
assert.strictEqual(server.address(), null);
assert.strictEqual(typeof server.listen, "function");
assert.strictEqual(typeof server.close, "function");
assert.strictEqual(typeof server.ref, "function");
assert.strictEqual(typeof server.unref, "function");
assert.strictEqual(server.unref(), server);
server.close();

console.log("net server shape passed");
