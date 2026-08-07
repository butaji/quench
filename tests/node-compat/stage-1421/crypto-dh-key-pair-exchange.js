const assert = require("node:assert");
const crypto = require("node:crypto");

const alice = crypto.generateKeyPairSync("dh", { group: "modp5" });
const bob = crypto.generateKeyPairSync("dh", { group: "modp5" });
assert.doesNotThrow(() =>
  crypto.diffieHellman({
    privateKey: alice.privateKey,
    publicKey: bob.publicKey,
  })
);
console.log("crypto DH key-pair exchange passed");
