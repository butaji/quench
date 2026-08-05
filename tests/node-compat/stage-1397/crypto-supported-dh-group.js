const assert = require("node:assert");
const crypto = require("node:crypto");

const group = crypto.getDiffieHellman("modp14");
assert.strictEqual(group.getPrime().byteLength, 128);
assert.deepStrictEqual([...group.getGenerator()], [2]);
console.log("crypto supported DH group passed");
