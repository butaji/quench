const assert = require("node:assert");
const crypto = require("node:crypto");

const pair = crypto.generateKeyPairSync("x25519");
assert.strictEqual(pair.privateKey.type, "private");
assert.strictEqual(pair.publicKey.type, "public");
console.log("crypto key-pair handles passed");
