"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

for (
  const name of [
    "createHash",
    "createHmac",
    "randomBytes",
    "randomFill",
    "randomUUID",
    "getCiphers",
    "getHashes",
  ]
) {
  assert.strictEqual(typeof crypto[name], "function");
}
assert.strictEqual(typeof crypto.constants, "object");
assert.strictEqual(typeof crypto.webcrypto, "object");
assert.strictEqual(typeof crypto.randomUUID(), "string");

console.log("crypto api passed");
