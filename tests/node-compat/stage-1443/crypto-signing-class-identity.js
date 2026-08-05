const assert = require("node:assert");
const crypto = require("node:crypto");

assert(crypto.createSign("RSA-SHA1") instanceof crypto.Sign);
assert(crypto.createVerify("RSA-SHA1") instanceof crypto.Verify);
console.log("crypto signing class identity passed");
