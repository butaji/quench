const assert = require("node:assert");
const crypto = require("node:crypto");

const cipher = crypto.createCipheriv(
  "aes-128-cbc",
  Buffer.alloc(16),
  Buffer.alloc(16),
);
cipher.update("data");
assert.throws(() => cipher.setAAD(Buffer.from("aad")), /state/);
console.log("crypto authenticated state passed");
