const assert = require("assert");
const crypto = require("crypto");

const digest = crypto.createHash("sha512").update("Test123").digest();
assert.strictEqual(digest.length, 64);
assert.strictEqual(
  digest.toString("hex"),
  "c12834f1031f6497214f27d4432f26517ad494156cb88d512bdb1dc4b57db2d692a3dfa269a19b0a0a2a0fd7d6a2a885e33c839c93c206da30a187392847ed27",
);

assert.throws(
  () => crypto.createHmac("sha7", "key"),
  { code: "ERR_CRYPTO_INVALID_DIGEST", name: "TypeError" },
);
