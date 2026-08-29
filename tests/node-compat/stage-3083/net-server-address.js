"use strict";

const assert = require("assert");
const net = require("net");

const server = net.createServer();
server.listen(0, () => {
  const address = server.address();
  assert.ok(address);
  assert.strictEqual(typeof address.port, "number");
  server.close();
});
