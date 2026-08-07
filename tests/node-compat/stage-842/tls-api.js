"use strict";

const assert = require("assert");
const tls = require("node:tls");

for (
  const name of [
    "connect",
    "createServer",
    "createSecureContext",
    "getCiphers",
    "checkServerIdentity",
  ]
) {
  assert.strictEqual(typeof tls[name], "function");
}
for (const name of ["Server", "TLSSocket", "SecureContext"]) {
  assert.strictEqual(typeof tls[name], "function");
}
assert.strictEqual(typeof tls.DEFAULT_MIN_VERSION, "string");

console.log("tls api passed");
