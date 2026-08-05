const assert = require("node:assert");
const crypto = require("node:crypto");

const secret = crypto.createSecretKey(Buffer.alloc(16));
assert(secret instanceof crypto.KeyObject);
assert.strictEqual(secret.type, "secret");
assert.strictEqual(secret.symmetricKeySize, 16);
assert.throws(() => crypto.KeyObject.prototype.type.call({}), {
  code: "ERR_INVALID_THIS"
});
console.log("crypto key object brand basics passed");
