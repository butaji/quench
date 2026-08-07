const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.createCipheriv("aes-127", Buffer.alloc(16), null), {
  name: "Error",
  code: "ERR_CRYPTO_UNKNOWN_CIPHER",
  message: "Unknown cipher",
});
assert.throws(
  () => crypto.createCipheriv("aes-128-ecb", Buffer.alloc(17), null),
  /Invalid key length/,
);
console.log("crypto cipher key validation passed");
