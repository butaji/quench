const assert = require("node:assert");
const crypto = require("node:crypto");

const pair = crypto.generateKeyPairSync("rsa", {
  publicExponent: 3,
  modulusLength: 512,
  publicKeyEncoding: { type: "pkcs1", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
assert.strictEqual(typeof pair.publicKey, "string");
assert.strictEqual(typeof pair.privateKey, "string");
assert(pair.publicKey.includes("BEGIN RSA PUBLIC KEY"));
assert(pair.privateKey.includes("BEGIN PRIVATE KEY"));
console.log("crypto encoded key generation passed");
