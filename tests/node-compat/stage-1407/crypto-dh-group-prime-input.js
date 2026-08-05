const assert = require("node:assert");
const crypto = require("node:crypto");

const prime = crypto.getDiffieHellman("modp14").getPrime();
const dh = crypto.createDiffieHellman(prime);
assert.strictEqual(typeof dh.setPrivateKey, "function");
dh.setPrivateKey("01020304", "hex");
assert.deepStrictEqual([...dh.getPrivateKey()], [1, 2, 3, 4]);
console.log("crypto DH group-prime input passed");
