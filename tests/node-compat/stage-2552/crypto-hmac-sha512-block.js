const assert = require("assert");
const crypto = require("crypto");

const actual = crypto
  .createHmac("sha512", "key")
  .update("The quick brown fox jumps over the lazy dog")
  .digest("hex");
assert.strictEqual(
  actual,
  "b42af09057bac1e2d41708e48a902e09b5ff7f12ab428a4fe86653c73dd248fb82f948a549f7b791a5b41915ee4d1ec3935357e4e2317250d0372afa2ebeeb3a",
);
console.log("crypto HMAC SHA-512 block passed");
