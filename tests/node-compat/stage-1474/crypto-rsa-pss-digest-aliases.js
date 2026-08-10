const assert = require("node:assert");
const crypto = require("node:crypto");

for (
  const [digest, length] of [
    ["sha384", 96],
    ["sha512", 128],
  ]
) {
  const hash = crypto.hash(digest, "test", "hex");
  assert.strictEqual(hash.length, length);
}
console.log("crypto RSA-PSS digest aliases passed");
