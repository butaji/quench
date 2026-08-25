"use strict";
const assert = require("assert");
const net = require("net");
const server = net.createServer((socket) => {
  assert.strictEqual(socket.remoteFamily, "IPv4");
  socket.end();
  server.close();
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect(server.address().port, "127.0.0.1");
  assert.strictEqual(client.remoteAddress, undefined);
  assert.strictEqual(client.remoteFamily, undefined);
  client.on("connect", () => {
    assert.strictEqual(client.remoteFamily, "IPv4");
    client.end();
  });
});
