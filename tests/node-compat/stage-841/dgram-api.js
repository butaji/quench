"use strict";

const assert = require("assert");
const dgram = require("node:dgram");

for (const name of ["createSocket", "Socket", "createSocket"]) {
  assert.strictEqual(typeof dgram[name], "function");
}
assert.strictEqual(typeof dgram.SocketAddress, "function");

console.log("dgram api passed");
