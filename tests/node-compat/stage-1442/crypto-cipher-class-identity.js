const assert = require("node:assert");
const crypto = require("node:crypto");

const cipher = crypto.createCipheriv(
  "aes-128-cbc",
  Buffer.alloc(16),
  Buffer.alloc(16),
);
assert(cipher instanceof crypto.Cipheriv);
console.log("crypto cipher class identity passed");
