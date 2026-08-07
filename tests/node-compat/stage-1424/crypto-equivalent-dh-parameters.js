const assert = require("node:assert");
const crypto = require("node:crypto");

const group = crypto.getDiffieHellman("modp5");
const privateKey = crypto.generateKeyPairSync("dh", {
  group: "modp5",
}).privateKey;
const publicKey = crypto.generateKeyPairSync("dh", {
  prime: group.getPrime(),
}).publicKey;
assert.doesNotThrow(() => crypto.diffieHellman({ privateKey, publicKey }));
console.log("crypto equivalent DH parameters passed");
