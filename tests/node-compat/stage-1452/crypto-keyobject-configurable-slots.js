const assert = require("node:assert");
const crypto = require("node:crypto");

const secret = crypto.createSecretKey(Buffer.alloc(16));
const typeDescriptor = Object.getOwnPropertyDescriptor(
  crypto.KeyObject.prototype,
  "type",
);
assert.strictEqual(typeDescriptor.configurable, true);
const originalType = typeDescriptor;
Object.defineProperty(crypto.KeyObject.prototype, "type", {
  configurable: true,
  get: () => "public",
});
assert.strictEqual(secret.type, "public");
Object.defineProperty(crypto.KeyObject.prototype, "type", originalType);
assert.strictEqual(
  crypto.createHmac("sha256", secret).digest("hex").length,
  64,
);
console.log("crypto configurable key slots passed");
