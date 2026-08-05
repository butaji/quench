const assert = require("node:assert");
const crypto = require("node:crypto");

const sign = crypto.Sign("SHA256");
const verify = crypto.Verify("SHA256");
assert(sign instanceof crypto.Sign);
assert(verify instanceof crypto.Verify);
console.log("crypto sign constructors passed");
