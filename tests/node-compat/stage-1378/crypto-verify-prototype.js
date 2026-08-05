const assert = require("node:assert");
const crypto = require("node:crypto");

assert(crypto.createVerify("sha1") instanceof crypto.Verify);
console.log("crypto Verify prototype passed");
