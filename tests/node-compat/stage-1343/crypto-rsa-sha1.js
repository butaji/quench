const assert = require("node:assert");
const crypto = require("node:crypto");

assert(crypto.getHashes().includes("RSA-SHA1"));
assert.strictEqual(
  crypto.createHash("RSA-SHA1").update("compatibility").digest("hex"),
  crypto.createHash("sha1").update("compatibility").digest("hex"),
);
console.log("crypto RSA-SHA1 passed");
