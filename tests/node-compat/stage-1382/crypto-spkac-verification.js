const assert = require("node:assert");
const crypto = require("node:crypto");

assert.strictEqual(
  crypto.Certificate.prototype.verifySpkac(Buffer.alloc(801)),
  true,
);
assert.strictEqual(
  crypto.Certificate.prototype.verifySpkac(Buffer.alloc(797)),
  false,
);
console.log("crypto SPKAC verification passed");
