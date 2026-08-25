"use strict";
const assert = require("assert");
const net = require("net");
const payload = "payload";
const server = net.createServer((socket) => {
  socket.end(payload);
  assert.strictEqual(socket.bytesWritten, payload.length);
  server.close();
});
server.listen(0, () => {
  const client = net.connect(server.address().port, () => client.resume());
  assert.strictEqual(client.bytesRead, 0);
  client.on("data", () => assert.ok(client.bytesRead > 0));
  client.on("close", () => assert.strictEqual(client.bytesRead, payload.length));
});
