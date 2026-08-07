const assert = require("node:assert");
const crypto = require("node:crypto");

const cipher = crypto.createCipheriv(
  "aes-128-cbc",
  Buffer.alloc(16),
  Buffer.alloc(16),
);
assert.strictEqual(cipher.setAAD(Buffer.from("aad")), cipher);
assert(cipher.getAuthTag() instanceof Uint8Array);
const decipher = crypto.createDecipheriv(
  "aes-128-cbc",
  Buffer.alloc(16),
  Buffer.alloc(16),
);
const tag = Buffer.alloc(16);
assert.strictEqual(decipher.setAuthTag(tag), decipher);
assert.throws(() => decipher.setAuthTag(tag), /state/);
console.log("crypto authenticated methods passed");
