const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(
  () => crypto.createCipheriv("aes-128-cbc", Buffer.alloc(16), null),
  /Invalid initialization vector/,
);
console.log("crypto CBC null IV validation passed");
