const assert = require("node:assert");
const crypto = require("node:crypto");

const privateKey = crypto.generateKeyPairSync("x448").privateKey;
const publicKey = crypto.generateKeyPairSync("x25519").publicKey;
assert.throws(() => crypto.diffieHellman({ privateKey, publicKey }), {
  code: "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
});
console.log("crypto EVP key-type mismatch passed");
