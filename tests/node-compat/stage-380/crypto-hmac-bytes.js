const assert = require("assert");
const crypto = require("crypto");
const digest = crypto
  .createHmac("sha256", Buffer.from("key"))
  .update(Buffer.from("The quick brown fox jumps over the lazy dog"))
  .digest("hex");
assert.strictEqual(
  digest,
  "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8",
);
