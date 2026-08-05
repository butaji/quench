const assert = require("node:assert");
const crypto = require("node:crypto");

const dh = crypto.createDiffieHellman(1024);
dh.setPrivateKey(Buffer.alloc(128, 1));
assert.strictEqual(dh.computeSecret(Buffer.alloc(128)).byteLength, 256);
console.log("crypto DH padding vector passed");
