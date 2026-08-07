const assert = require("node:assert");
const crypto = require("node:crypto");

for (const authTagLength of [0, 17]) {
  assert.throws(
    () =>
      crypto.createCipheriv(
        "chacha20-poly1305",
        Buffer.alloc(32),
        Buffer.alloc(12),
        {
          authTagLength,
        },
      ),
    { code: "ERR_CRYPTO_INVALID_AUTH_TAG" },
  );
}
console.log("crypto auth tag length passed");
