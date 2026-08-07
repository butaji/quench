const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.diffieHellman({ privateKey: {} }), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => crypto.diffieHellman({ publicKey: {} }), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("crypto stateless DH required keys passed");
