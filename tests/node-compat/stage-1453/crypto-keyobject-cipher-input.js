const assert = require("node:assert");
const crypto = require("node:crypto");

const secret = crypto.createSecretKey(Buffer.alloc(16));
const descriptor = Object.getOwnPropertyDescriptor(
  Object.getPrototypeOf(secret),
  "symmetricKeySize",
);
Object.defineProperty(Object.getPrototypeOf(secret), "symmetricKeySize", {
  configurable: true,
  get: () => 1,
});
try {
  const cipher = crypto.createCipheriv("aes-128-ecb", secret, null);
  assert(cipher);
} finally {
  Object.defineProperty(
    Object.getPrototypeOf(secret),
    "symmetricKeySize",
    descriptor,
  );
}
console.log("crypto KeyObject cipher input passed");
