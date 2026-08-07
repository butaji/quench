const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.createDiffieHellman("abcdef", "hex", -1), {
  code: "ERR_OSSL_DH_BAD_GENERATOR",
});
console.log("crypto DH encoding overload passed");
