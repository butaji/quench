const assert = require("node:assert");
const crypto = require("node:crypto");

const cipher = crypto.createCipheriv("des-ede3-cbc", "key", "iv");
const encrypted = cipher.update("buffer compatibility", "utf8", "buffer");
const decipher = crypto.createDecipheriv("des-ede3-cbc", "key", "iv");
const decrypted = decipher.update(encrypted, "buffer", "utf8");
assert.strictEqual(decrypted, "buffer compatibility");
console.log("crypto cipher buffer round trip passed");
