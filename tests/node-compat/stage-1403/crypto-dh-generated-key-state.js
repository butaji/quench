const assert = require("node:assert");
const crypto = require("node:crypto");

const dh = crypto.createDiffieHellman(1024);
const first = dh.generateKeys();
const second = dh.generateKeys();
assert.deepStrictEqual(first, second);
dh.setPrivateKey(Buffer.from("01020304", "hex"));
assert.notDeepStrictEqual(second, dh.generateKeys());
assert.strictEqual(dh.getPrivateKey().byteLength, 4);
console.log("crypto DH generated-key state passed");
