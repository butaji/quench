const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.createCipheriv("aes-128-ecb", Buffer.alloc(16)), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(
  () => crypto.createCipheriv("aes-128-ecb", Buffer.alloc(16), undefined),
  { code: "ERR_INVALID_ARG_TYPE" },
);
console.log("crypto missing IV validation passed");
