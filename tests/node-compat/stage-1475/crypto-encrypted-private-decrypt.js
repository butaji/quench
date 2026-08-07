const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(
  () =>
    crypto.privateDecrypt(
      "-----BEGIN ENCRYPTED PRIVATE KEY-----",
      Buffer.alloc(1),
    ),
  {
    message:
      "error:07880109:common libcrypto routines::interrupted or cancelled",
  },
);
console.log("crypto encrypted private decrypt passed");
