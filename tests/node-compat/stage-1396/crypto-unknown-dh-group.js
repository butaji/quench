const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.getDiffieHellman("unknown-group"), {
  code: "ERR_CRYPTO_UNKNOWN_DH_GROUP",
  message: "Unknown DH group",
});
console.log("crypto unknown DH group passed");
