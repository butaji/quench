const assert = require("node:assert");
const crypto = require("node:crypto");

crypto.createCipheriv("aes-128-cbc", Buffer.alloc(16), Buffer.alloc(16));
crypto.createCipheriv("des-ede3-cbc", Buffer.alloc(24), Buffer.alloc(8));
for (const length of [0, 1, 15, 17]) {
  assert.throws(
    () =>
      crypto.createCipheriv(
        "aes-128-cbc",
        Buffer.alloc(16),
        Buffer.alloc(length),
      ),
    /Invalid initialization vector/,
  );
}
console.log("crypto CBC IV validation passed");
