const assert = require("node:assert");
const crypto = require("node:crypto");

const decipher = crypto.createDecipheriv(
  "chacha20-poly1305",
  Buffer.alloc(32),
  Buffer.alloc(12),
  { authTagLength: 16 },
);
decipher.update(Buffer.alloc(1));
assert.throws(
  () => decipher.final(),
  /Unsupported state or unable to authenticate data/,
);
console.log("crypto authentication final state passed");
