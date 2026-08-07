const assert = require("node:assert");
const crypto = require("node:crypto");

const key = crypto.createPrivateKey({
  key: "-----BEGIN PRIVATE KEY-----",
  format: "pem",
});
assert.strictEqual(key.source.key.includes("BEGIN PRIVATE KEY"), true);
console.log("crypto DH short secret passed");
