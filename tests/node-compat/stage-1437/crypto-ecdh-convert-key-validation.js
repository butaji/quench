const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.ECDH.convertKey(), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => crypto.ECDH.convertKey("abcd", "badcurve"), {
  message: "Invalid EC curve name",
});
console.log("crypto ECDH convert-key validation passed");
