const assert = require("node:assert");
const crypto = require("node:crypto");

const { publicKey, privateKey } = crypto.generateKeyPairSync("rsa", {
  modulusLength: 512,
});
for (const key of [publicKey, privateKey]) {
  assert.deepStrictEqual(key.asymmetricKeyDetails, {
    modulusLength: 512,
    publicExponent: 65537n,
  });
}
console.log("crypto RSA key details passed");
