const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(
  () =>
    crypto.diffieHellman({
      privateKey: crypto.generateKeySync("aes", { length: 128 }),
    }),
  { code: "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE" },
);
console.log("crypto stateless DH key types passed");
