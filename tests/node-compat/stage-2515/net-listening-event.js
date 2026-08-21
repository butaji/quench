const assert = require("assert");
const net = require("net");

const server = net.createServer();
server.once("listening", () => {
  assert.strictEqual(server.listening, true);
  server.close();
});
server.listen(0);
