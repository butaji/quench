const assert = require("node:assert");
const crypto = require("node:crypto");

const ecdh = crypto.createECDH("prime256v1");
assert(ecdh instanceof crypto.ECDH);
console.log("crypto ECDH class identity passed");
