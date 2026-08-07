const assert = require("node:assert");
const crypto = require("node:crypto");

const pair = crypto.generateKeyPairSync("rsa", {
  modulusLength: 512,
  publicKeyEncoding: { type: "pkcs1", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const message = Buffer.from("Hello Node.js world!");
const signature = crypto.sign("SHA256", message, pair.privateKey);
assert.strictEqual(signature.length, 64);
assert.strictEqual(
  crypto.verify("SHA256", message, pair.publicKey, signature),
  true,
);
console.log("crypto direct signing passed");
