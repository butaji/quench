const assert = require("node:assert");
const crypto = require("node:crypto");

const actual = crypto
  .createHmac("sha1", "secret")
  .update("message")
  .digest("hex");
assert.strictEqual(actual, "0caf649feee4953d87bf903ac1176c45e028df16");
console.log("crypto HMAC SHA-1 passed");
