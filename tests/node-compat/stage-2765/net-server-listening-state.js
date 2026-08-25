"use strict";
const assert = require("assert");
const net = require("net");
const server = net.createServer();
assert.strictEqual(server.listening, false);
assert.strictEqual(Object.prototype.propertyIsEnumerable.call(server, "listening"), false);
server.listen(0, () => {
  assert.strictEqual(server.listening, true);
  server.close(() => assert.strictEqual(server.listening, false));
});
