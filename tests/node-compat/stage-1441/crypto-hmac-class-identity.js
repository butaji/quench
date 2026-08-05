const assert = require("node:assert");
const crypto = require("node:crypto");

assert(crypto.createHmac("sha1", "key") instanceof crypto.Hmac);
console.log("crypto Hmac class identity passed");
