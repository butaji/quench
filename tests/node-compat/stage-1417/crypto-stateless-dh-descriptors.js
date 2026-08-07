const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(
  () =>
    crypto.diffieHellman({
      privateKey: { key: Buffer.alloc(0), format: "banana", type: "pkcs8" },
      publicKey: "pem",
    }),
  { code: "ERR_INVALID_ARG_VALUE", message: /options\.privateKey\.format/ },
);
console.log("crypto stateless DH descriptors passed");
