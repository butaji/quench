const assert = require("node:assert");
const crypto = require("node:crypto");

const dh = crypto.createDiffieHellman(1024);
assert.strictEqual(typeof dh.setPrivateKey, "function");
dh.setPrivateKey("01020304", "hex");
assert.deepStrictEqual([...dh.getPrivateKey()], [1, 2, 3, 4]);
console.log("crypto DH encoded private key passed");
