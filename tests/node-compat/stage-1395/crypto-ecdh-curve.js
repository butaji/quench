const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.createECDH(), {
  code: "ERR_INVALID_ARG_TYPE",
  message: 'The "curve" argument must be of type string. Received undefined',
});
console.log("crypto ECDH curve validation passed");
