const assert = require("node:assert");
const crypto = require("node:crypto");

const signer = crypto.createSign("sha256").update("plaintext");
assert.strictEqual(
  signer.sign("-----BEGIN EC PRIVATE KEY-----").byteLength,
  64,
);
console.log("crypto EC signing passed");
