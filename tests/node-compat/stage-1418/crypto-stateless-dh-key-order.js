const assert = require("node:assert");
const crypto = require("node:crypto");

const pair = crypto.generateKeyPairSync("x25519");
assert.throws(
  () =>
    crypto.diffieHellman({
      privateKey: pair.publicKey,
      publicKey: pair.publicKey,
    }),
  { code: "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE" },
);
console.log("crypto stateless DH key ordering passed");
