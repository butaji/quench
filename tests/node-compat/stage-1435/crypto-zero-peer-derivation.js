const assert = require("node:assert");
const crypto = require("node:crypto");

const peer = crypto.createPublicKey("AAAAAAAA");
assert.throws(
  () =>
    crypto.diffieHellman({ privateKey: { type: "private" }, publicKey: peer }),
  { code: "ERR_OSSL_FAILED_DURING_DERIVATION" },
);
console.log("crypto zero peer derivation passed");
