const assert = require("node:assert");
const crypto = require("node:crypto");

const privateKey = crypto.generateKeyPairSync("ec", {
  namedCurve: "P-256",
}).privateKey;
const publicKey = crypto.generateKeyPairSync("ec", {
  namedCurve: "P-384",
}).publicKey;
assert.throws(() => crypto.diffieHellman({ privateKey, publicKey }), {
  code: "ERR_OSSL_MISMATCHING_DOMAIN_PARAMETERS",
});
console.log("crypto EC domain mismatch passed");
