const assert = require("node:assert");
const crypto = require("node:crypto");

assert(crypto.createSign("sha1") instanceof crypto.Sign);
console.log("crypto Sign prototype passed");
