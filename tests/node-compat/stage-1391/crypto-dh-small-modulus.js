const assert = require("node:assert");
const crypto = require("node:crypto");

for (const bits of [-1, 0, 1]) {
  assert.throws(() => crypto.createDiffieHellman(bits), {
    code: "ERR_OSSL_DH_MODULUS_TOO_SMALL",
    name: "Error",
  });
}
console.log("crypto DH small modulus passed");
