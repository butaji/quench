"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

for (
  const name of [
    "createSecretKey",
    "createPublicKey",
    "createPrivateKey",
    "createDiffieHellman",
    "createECDH",
    "KeyObject",
    "Certificate",
    "X509Certificate",
  ]
) {
  assert.strictEqual(typeof crypto[name], "function");
}

console.log("crypto keys api passed");
