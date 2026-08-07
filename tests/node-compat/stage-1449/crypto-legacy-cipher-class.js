const assert = require("node:assert");
const crypto = require("node:crypto");

const cipher = crypto.createCipheriv(
  "des-ede3-cbc",
  "0123456789abcd0123456789",
  "12345678",
);
assert(cipher instanceof crypto.Cipheriv);
console.log("crypto legacy cipher class passed");
