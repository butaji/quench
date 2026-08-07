const assert = require("assert");
const net = require("net");

const server = net.createServer();
assert.strictEqual(server instanceof net.Server, true);
assert.strictEqual(server.constructor, net.Server);
server.close();

console.log("net Server instance passed");
