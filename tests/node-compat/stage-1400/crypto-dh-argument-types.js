const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.createDiffieHellman("", true), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => crypto.createDiffieHellman("", "base64", []), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("crypto DH argument types passed");
