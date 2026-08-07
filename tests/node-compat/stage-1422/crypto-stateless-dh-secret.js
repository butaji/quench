const assert = require("node:assert");
const crypto = require("node:crypto");

const alice = crypto.generateKeyPairSync("dh", { group: "modp5" });
const bob = crypto.generateKeyPairSync("dh", { group: "modp5" });
const oneShot = crypto.diffieHellman({
  privateKey: alice.privateKey,
  publicKey: bob.publicKey,
});
assert.strictEqual(oneShot.byteLength, 256);
console.log("crypto stateless DH secret passed");
