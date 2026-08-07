"use strict";

const assert = require("assert");
const net = require("node:net");

for (
  const name of [
    "createServer",
    "createConnection",
    "connect",
    "isIP",
    "isIPv4",
    "isIPv6",
  ]
) {
  assert.strictEqual(typeof net[name], "function");
}
for (const name of ["Server", "Socket", "SocketAddress"]) {
  assert.strictEqual(typeof net[name], "function");
}
assert.strictEqual(typeof net.BlockList, "function");

console.log("net api passed");
