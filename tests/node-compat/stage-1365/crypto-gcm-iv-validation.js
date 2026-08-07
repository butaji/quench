const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(
  () => crypto.createCipheriv("aes-128-gcm", Buffer.alloc(16), Buffer.alloc(0)),
  /Invalid initialization vector/,
);
for (const length of [8, 16, 64]) {
  crypto.createCipheriv("aes-128-gcm", Buffer.alloc(16), Buffer.alloc(length));
}
assert.throws(
  () =>
    crypto.createCipheriv("aes-128-gcm", Buffer.alloc(16), Buffer.alloc(65)),
  /Invalid initialization vector/,
);
console.log("crypto GCM IV validation passed");
