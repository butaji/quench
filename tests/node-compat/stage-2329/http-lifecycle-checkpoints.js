const assert = require("assert");
const http = require("http");

assert.strictEqual(typeof http.createServer, "function");
const server = http.createServer();
assert.ok(server);
assert.strictEqual(server.listening, false);
server.listen(0, () => {
  assert.strictEqual(server.listening, true);
  server.close(() => console.log("http lifecycle checkpoints passed"));
});
