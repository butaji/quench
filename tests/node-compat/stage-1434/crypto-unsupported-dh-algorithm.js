const assert = require("node:assert");
const crypto = require("node:crypto");

const privateKey = crypto.generateKeyPairSync("ed25519").privateKey;
const publicKey = crypto.generateKeyPairSync("ed25519").publicKey;
assert.throws(() => crypto.diffieHellman({ privateKey, publicKey }), {
  code: "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
});
console.log("crypto unsupported DH algorithm passed");
