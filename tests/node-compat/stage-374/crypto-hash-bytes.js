const assert = require("assert");
const crypto = require("crypto");
const digest = crypto
  .createHash("sha256")
  .update(Buffer.from("abc"))
  .digest("hex");
assert.strictEqual(
  digest,
  "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
);
