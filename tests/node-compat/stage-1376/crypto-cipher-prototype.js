const assert = require("node:assert");
const crypto = require("node:crypto");

assert(
  crypto.createCipheriv(
    "aes-128-cbc",
    Buffer.alloc(16),
    Buffer.alloc(16),
  ) instanceof crypto.Cipheriv,
);
console.log("crypto Cipheriv prototype passed");
