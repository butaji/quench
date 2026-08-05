const assert = require("node:assert");
const crypto = require("node:crypto");

const dh = crypto.createDiffieHellman(1024);
assert.strictEqual(typeof dh.setPublicKey, "function");
dh.setPublicKey("01020304", "hex");
assert.deepStrictEqual([...dh.getPublicKey()], [1, 2, 3, 4]);
console.log("crypto DH encoded public key passed");
