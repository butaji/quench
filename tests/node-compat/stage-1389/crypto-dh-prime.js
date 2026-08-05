const assert = require("node:assert");
const crypto = require("node:crypto");

const prime = crypto.createDiffieHellman(1024).getPrime("buffer");
assert(prime instanceof Uint8Array);
assert.strictEqual(prime.length, 128);
console.log("crypto DH prime passed");
