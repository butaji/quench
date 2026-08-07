const assert = require("node:assert");
const crypto = require("node:crypto");

assert.strictEqual(
  typeof crypto.createPrivateKey("private").export,
  "function",
);
assert.strictEqual(typeof crypto.createPublicKey("public").export, "function");
console.log("crypto key export passed");
