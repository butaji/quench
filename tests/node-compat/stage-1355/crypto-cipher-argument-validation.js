const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.createCipheriv(null), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
assert.throws(() => crypto.createCipheriv("des-ede3-cbc", null), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
assert.throws(() => crypto.createCipheriv("des-ede3-cbc", "key", 10), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
console.log("crypto cipher argument validation passed");
