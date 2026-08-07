const assert = require("node:assert");
const crypto = require("node:crypto");

const alice = crypto.createDiffieHellmanGroup("modp5");
const bob = crypto.createDiffieHellmanGroup("modp5");
alice.generateKeys();
bob.generateKeys();
assert.strictEqual(
  alice.computeSecret(bob.getPublicKey()).toString("hex"),
  bob.computeSecret(alice.getPublicKey()).toString("hex"),
);
console.log("crypto DH group factory passed");
