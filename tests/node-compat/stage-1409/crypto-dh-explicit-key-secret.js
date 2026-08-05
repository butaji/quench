const assert = require("node:assert");
const crypto = require("node:crypto");

const dh = crypto.createDiffieHellman(1024);
dh.setPrivateKey("01020304", "hex");
dh.setPublicKey("05060708", "hex");
assert.strictEqual(dh.computeSecret(Buffer.alloc(4)).byteLength, 128);
console.log("crypto DH explicit-key secret passed");
