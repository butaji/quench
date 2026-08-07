const assert = require("node:assert");
const crypto = require("node:crypto");

crypto.createCipheriv("aes-128-ecb", crypto.randomBytes(16), null);
crypto.createCipheriv("aes-128-ecb", crypto.randomBytes(16), Buffer.alloc(0));
assert.throws(
  () =>
    crypto.createCipheriv(
      "aes-128-ecb",
      crypto.randomBytes(16),
      Buffer.alloc(1),
    ),
  /Invalid initialization vector/,
);
console.log("crypto ECB IV validation passed");
