const assert = require("node:assert");
const crypto = require("node:crypto");

const group = crypto.getDiffieHellman("modp14");
const alice = crypto.createDiffieHellman(
  group.getPrime(),
  group.getGenerator(),
);
assert.throws(() => alice.computeSecret(Buffer.alloc(128)), {
  code: "ERR_CRYPTO_INVALID_STATE",
});
assert.strictEqual(alice.generateKeys().byteLength, 128);
assert.strictEqual(alice.getPublicKey().byteLength, 128);
assert.strictEqual(alice.computeSecret(Buffer.alloc(128)).byteLength, 128);
console.log("crypto DH key methods passed");
