const assert = require("node:assert");
const crypto = require("node:crypto");

const createCipher = () =>
  crypto.createCipheriv(
    "aes-256-cbc",
    crypto.randomBytes(32),
    crypto.randomBytes(16),
  );

const cipher = createCipher();
cipher.update("test", "utf-8", "utf-8");
assert.throws(() => cipher.update("666f6f", "hex", "hex"), {
  code: "ERR_INVALID_ARG_VALUE",
  name: "TypeError",
});
assert.throws(() => cipher.final("hex"), {
  code: "ERR_INVALID_ARG_VALUE",
  name: "TypeError",
});
console.log("crypto cipher encoding validation passed");
