const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.diffieHellman(), {
  code: "ERR_INVALID_ARG_TYPE",
  message: 'The "options" argument must be of type object. Received undefined',
});
assert.throws(() => crypto.diffieHellman(null), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("crypto stateless DH options passed");
