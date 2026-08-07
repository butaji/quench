"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

for (
  const name of [
    "sign",
    "verify",
    "createSign",
    "createVerify",
    "generateKeyPair",
    "generateKeyPairSync",
    "generateKey",
    "generateKeySync",
    "createPrivateKey",
    "createPublicKey",
  ]
) {
  assert.strictEqual(typeof crypto[name], "function");
}

console.log("crypto signing api passed");
