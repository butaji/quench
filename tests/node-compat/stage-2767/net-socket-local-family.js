"use strict";
const assert = require("assert");
const net = require("net");
const server = net.createServer((socket) => {
  assert.strictEqual(socket.localFamily, "IPv4");
  server.close();
  socket.destroy();
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect(server.address().port, "127.0.0.1");
  client.on("connect", () => client.end());
});
