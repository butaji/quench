const assert = require("node:assert");
const crypto = require("node:crypto");

const decipher = crypto.createDecipheriv(
  "aes-128-cbc",
  Buffer.alloc(16),
  Buffer.alloc(16),
);
assert(decipher instanceof crypto.Decipheriv);
console.log("crypto decipher class identity passed");
