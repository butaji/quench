const assert = require("node:assert");
const crypto = require("node:crypto");

const encoded = crypto.publicEncrypt(
  { key: "key", encoding: "hex" },
  Buffer.from("I AM THE WALRUS").toString("hex"),
);
assert.strictEqual(
  crypto.privateDecrypt("key", encoded).toString(),
  "I AM THE WALRUS",
);
console.log("crypto hex encryption input passed");
