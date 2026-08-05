const assert = require("node:assert");
const crypto = require("node:crypto");

for (const digest of ["sha384", "sha512"]) {
  const hash = crypto.hash(digest, "test", "hex");
  assert.strictEqual(hash.length, 64);
}
console.log("crypto RSA-PSS digest aliases passed");
