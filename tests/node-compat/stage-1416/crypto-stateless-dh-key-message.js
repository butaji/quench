const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(
  () =>
    crypto.diffieHellman({
      privateKey: { type: "private" },
      publicKey: { type: "secret" },
    }),
  { message: "Invalid key object type secret, expected private or public." },
);
console.log("crypto stateless DH key message passed");
