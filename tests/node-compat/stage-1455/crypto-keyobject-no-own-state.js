const assert = require("node:assert");
const crypto = require("node:crypto");

const secret = crypto.createSecretKey(Buffer.alloc(16));
assert.strictEqual(secret.equals(secret), true);
assert.deepStrictEqual(Reflect.ownKeys(secret), []);
assert.strictEqual(secret.type, "secret");
assert.strictEqual(secret.symmetricKeySize, 16);
console.log("crypto KeyObject own state passed");
