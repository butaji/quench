const assert = require("node:assert");
const crypto = require("node:crypto");

assert(crypto.createHash("sha1") instanceof crypto.Hash);
console.log("crypto Hash class identity passed");
