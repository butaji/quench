const assert = require("node:assert");
const crypto = require("node:crypto");

const certificate = new crypto.Certificate();
assert.strictEqual(
  certificate.exportChallenge(Buffer.alloc(801)).toString(),
  "this-is-a-challenge",
);
assert(
  certificate.exportPublicKey(Buffer.alloc(801)).includes("BEGIN PUBLIC KEY"),
);
console.log("crypto SPKAC exports passed");
