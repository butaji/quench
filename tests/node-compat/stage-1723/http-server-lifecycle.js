const assert = require("node:assert");
const http = require("node:http");

const server = http.createServer();
assert.strictEqual(server.closeAllConnections(), server);
assert.strictEqual(server.closeIdleConnections(), server);
assert.strictEqual(typeof server[Symbol.asyncDispose], "function");
server.listen(43211);
server[Symbol.asyncDispose]().then(() => {
  assert.strictEqual(server._port, 43211);
  console.log("http server lifecycle passed");
});
