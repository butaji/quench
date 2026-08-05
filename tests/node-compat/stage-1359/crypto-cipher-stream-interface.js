const assert = require("node:assert");
const crypto = require("node:crypto");

const cipher = crypto.createCipheriv("des-ede3-cbc", "key", "iv");
cipher.end("stream value");
assert(cipher.readableLength > 0);
assert(cipher.read() instanceof Uint8Array);
console.log("crypto cipher stream interface passed");
