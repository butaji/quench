const assert = require("node:assert");
const crypto = require("node:crypto");

assert(crypto.getHashes().includes("sha1"));
assert(crypto.getHashes().includes("sha256"));
console.log("crypto hash list passed");
