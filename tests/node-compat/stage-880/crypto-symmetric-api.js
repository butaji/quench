"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

for (
  const name of [
    "createCipheriv",
    "createDecipheriv",
    "createDiffieHellman",
    "hkdf",
    "hkdfSync",
    "pbkdf2",
    "pbkdf2Sync",
    "scrypt",
    "scryptSync",
    "randomInt",
  ]
) {
  assert.strictEqual(typeof crypto[name], "function");
}

console.log("crypto symmetric api passed");
