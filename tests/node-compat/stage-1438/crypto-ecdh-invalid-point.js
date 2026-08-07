const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(
  () =>
    crypto.ECDH.convertKey(
      "f".repeat(128),
      "secp521r1",
      "hex",
      "hex",
      "compressed",
    ),
  /Failed to convert Buffer to EC_POINT/,
);
console.log("crypto ECDH invalid point passed");
