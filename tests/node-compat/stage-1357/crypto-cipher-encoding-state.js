const assert = require("node:assert");
const crypto = require("node:crypto");

const cipher = crypto.createCipheriv(
  "aes-256-cbc",
  crypto.randomBytes(32),
  crypto.randomBytes(16),
);
assert.doesNotThrow(() => cipher.update("test", "utf8", "hex"));
assert.throws(() => cipher.update("666f6f", "hex", "hex"), {
  code: "ERR_INVALID_ARG_VALUE",
});
console.log("crypto cipher encoding state passed");
