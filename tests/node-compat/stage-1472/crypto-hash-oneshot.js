const assert = require("node:assert");
const crypto = require("node:crypto");

assert.strictEqual(
  crypto.hash("sha1", "test", "hex"),
  crypto.createHash("sha1").update("test").digest("hex"),
);
assert.throws(() => crypto.hash(1, "test"), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => crypto.hash("sha1", "test", 0), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("crypto one-shot hash passed");
