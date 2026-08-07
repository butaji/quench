const assert = require("node:assert");
const crypto = require("node:crypto");

const privateKey = crypto.generateKeyPairSync("dh", {
  group: "modp5",
}).privateKey;
const publicKey = crypto.generateKeyPairSync("dh", {
  group: "modp18",
}).publicKey;
assert.throws(() => crypto.diffieHellman({ privateKey, publicKey }), {
  code: "ERR_OSSL_MISMATCHING_DOMAIN_PARAMETERS",
});
console.log("crypto DH parameter mismatch passed");
