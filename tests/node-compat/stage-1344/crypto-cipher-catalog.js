const assert = require("node:assert");
const crypto = require("node:crypto");

assert.deepStrictEqual(crypto.getCiphers(), ["aes-128-cbc"]);
assert.deepStrictEqual(crypto.getCipherInfo("aes-128-cbc"), {
  name: "aes-128-cbc",
  nid: 419,
  blockSize: 16,
  ivLength: 16,
  keyLength: 16,
  mode: "cbc",
});
console.log("crypto cipher catalog passed");
