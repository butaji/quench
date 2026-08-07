const assert = require("node:assert");
const crypto = require("node:crypto");

const dh = crypto.createDiffieHellman(1024);
assert.throws(() => dh.getPrivateKey(), {
  code: "ERR_CRYPTO_INVALID_STATE",
});
dh.setPrivateKey(Buffer.from("01020304", "hex"));
assert.deepStrictEqual([...dh.getPrivateKey()], [1, 2, 3, 4]);
console.log("crypto DH private-key access passed");
