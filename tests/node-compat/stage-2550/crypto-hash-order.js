const assert = require("assert");
const crypto = require("crypto");

assert.deepStrictEqual(crypto.getHashes(), [
  "RSA-SHA1",
  "md5",
  "sha1",
  "sha224",
  "sha256",
  "sha384",
  "sha512",
]);
console.log("crypto hash order passed");
