const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.generateKeySync("aes", { length: 123 }), {
  code: "ERR_INVALID_ARG_VALUE",
});
assert.throws(() => crypto.generateKeySync("hmac", { length: 4 }), {
  code: "ERR_OUT_OF_RANGE",
});
assert.strictEqual(
  crypto.generateKeySync("aes", { length: 128 }).export().byteLength,
  16,
);
console.log("crypto secret key validation passed");
