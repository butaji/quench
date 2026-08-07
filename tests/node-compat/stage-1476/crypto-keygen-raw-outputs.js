const assert = require("node:assert");
const crypto = require("node:crypto");

const pair = crypto.generateKeyPairSync("ed25519", {
  publicKeyEncoding: { format: "raw-public" },
  privateKeyEncoding: { format: "raw-private" },
});
assert(Buffer.isBuffer(pair.publicKey));
assert(Buffer.isBuffer(pair.privateKey));
console.log("crypto raw key generation passed");
