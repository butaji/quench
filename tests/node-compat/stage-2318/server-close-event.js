const assert = require("assert");
const net = require("net");

const server = net.createServer();
let closed = false;
server.once("close", () => {
  closed = true;
});
server.listen(0, () => {
  assert.strictEqual(server.close(), server);
  queueMicrotask(() => assert.ok(closed));
});
