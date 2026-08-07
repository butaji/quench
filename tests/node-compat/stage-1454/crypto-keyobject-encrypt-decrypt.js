const assert = require("node:assert");
const crypto = require("node:crypto");

const secret = crypto.createSecretKey(Buffer.alloc(16));
const prototype = Object.getPrototypeOf(secret);
const original = Object.getOwnPropertyDescriptor(prototype, "symmetricKeySize");
Object.defineProperty(prototype, "symmetricKeySize", {
  configurable: true,
  get: () => 1,
});
try {
  const cipher = crypto.createCipheriv("aes-128-ecb", secret, null);
  const ciphertext = Buffer.concat([
    cipher.update(Buffer.alloc(16)),
    cipher.final(),
  ]);
  const decipher = crypto.createDecipheriv("aes-128-ecb", secret, null);
  const plaintext = Buffer.concat([
    decipher.update(ciphertext),
    decipher.final(),
  ]);
  assert.deepStrictEqual(plaintext, Buffer.alloc(16));
} finally {
  Object.defineProperty(prototype, "symmetricKeySize", original);
}
console.log("crypto KeyObject encrypt decrypt passed");
