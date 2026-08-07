const assert = require("node:assert");
const crypto = require("node:crypto");

const key = "0123456789abcd0123456789";
const iv = "12345678";
const plaintext = "cipher compatibility";
const cipher = crypto.createCipheriv("des-ede3-cbc", key, iv);
const encrypted = cipher.update(plaintext, "utf8", "hex") + cipher.final("hex");
const decipher = crypto.createDecipheriv("des-ede3-cbc", key, iv);
const decrypted = decipher.update(encrypted, "hex", "utf8") +
  decipher.final("utf8");
assert.strictEqual(decrypted, plaintext);
console.log("crypto cipher round trip passed");
