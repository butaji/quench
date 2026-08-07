const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.createDiffieHellman(13.37), {
  code: "ERR_OUT_OF_RANGE",
  name: "RangeError",
});
assert.throws(() => crypto.createDiffieHellman("abcdef", 13.37), {
  code: "ERR_OUT_OF_RANGE",
  name: "RangeError",
});
console.log("crypto DH number validation passed");
