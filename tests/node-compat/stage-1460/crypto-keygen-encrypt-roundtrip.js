const assert = require("node:assert");
const crypto = require("node:crypto");

const pair = crypto.generateKeyPairSync("rsa", {
  modulusLength: 512,
  publicKeyEncoding: { type: "pkcs1", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const message = Buffer.from("Hello Node.js world!");
const ciphertext = crypto.publicEncrypt(pair.publicKey, message);
assert.deepStrictEqual(
  crypto.privateDecrypt(pair.privateKey, ciphertext),
  message,
);
console.log("crypto encoded key encryption passed");
