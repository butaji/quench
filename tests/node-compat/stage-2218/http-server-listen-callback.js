const assert = require("assert");
const http = require("http");

const server = http.createServer();
server.listen(() => {
  assert.strictEqual(typeof server.address().port, "number");
  server.close();
});
