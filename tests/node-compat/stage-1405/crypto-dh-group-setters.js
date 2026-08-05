const assert = require("node:assert");
const crypto = require("node:crypto");

const group = crypto.getDiffieHellman("modp1");
assert.strictEqual(group.constructor, crypto.DiffieHellmanGroup);
assert.strictEqual(group.setPrivateKey, undefined);
assert.strictEqual(group.setPublicKey, undefined);
console.log("crypto DH group setters passed");
