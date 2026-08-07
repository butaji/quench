const assert = require("node:assert");
const crypto = require("node:crypto");
const { types } = require("node:util");

const secret = crypto.createSecretKey(Buffer.alloc(16));
const { publicKey } = crypto.generateKeyPairSync("rsa", {
  modulusLength: 1024,
});
assert.strictEqual(typeof types.isKeyObject, "function");
assert.strictEqual(types.isKeyObject(secret), true);
assert.strictEqual(types.isKeyObject(publicKey), true);
assert.strictEqual(types.isKeyObject({}), false);
console.log("crypto KeyObject util brand passed");
