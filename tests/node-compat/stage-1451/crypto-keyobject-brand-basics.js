const assert = require("node:assert");
const crypto = require("node:crypto");

const secret = crypto.createSecretKey(Buffer.alloc(16));
assert(secret instanceof crypto.KeyObject);
assert.strictEqual(secret.type, "secret");
assert.strictEqual(secret.symmetricKeySize, 16);
const originalType = Object.getOwnPropertyDescriptor(
  crypto.KeyObject.prototype,
  "type",
);
Object.defineProperty(crypto.KeyObject.prototype, "type", {
  configurable: true,
  get: () => "public",
});
assert.strictEqual(secret.type, "public");
Object.defineProperty(crypto.KeyObject.prototype, "type", originalType);
assert.throws(() => crypto.KeyObject.prototype.type.call({}), {
  code: "ERR_INVALID_THIS",
});
console.log("crypto key object brand basics passed");
