const assert = require("node:assert");
const crypto = require("crypto");
const nodeCrypto = require("node:crypto");

assert.strictEqual(typeof crypto.generateKeyPairSync, "function");
assert.strictEqual(typeof nodeCrypto.generateKeyPairSync, "function");
assert.strictEqual(typeof crypto.randomUUIDv7, "function");
assert.strictEqual(typeof nodeCrypto.randomUUIDv7, "function");
assert.strictEqual(typeof crypto.generatePrimeSync, "function");
assert.strictEqual(typeof nodeCrypto.generatePrimeSync, "function");
console.log("crypto shared module fallbacks passed");
