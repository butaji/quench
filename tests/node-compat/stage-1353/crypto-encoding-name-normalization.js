const assert = require("node:assert");
const crypto = require("node:crypto");

const cipher = crypto.createCipheriv(
  "aes-256-cbc",
  crypto.randomBytes(32),
  crypto.randomBytes(16),
);
cipher.update("test", "utf-8", "utf-8");
assert.throws(() => cipher.update("666f6f", "hex", "hex"), {
  code: "ERR_INVALID_ARG_VALUE",
  message: /cannot be changed from 'utf8'/,
});
console.log("crypto encoding name normalization passed");
