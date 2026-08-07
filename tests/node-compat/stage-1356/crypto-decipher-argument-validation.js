const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.createDecipheriv(null), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
assert.throws(() => crypto.createDecipheriv("des-ede3-cbc", null), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
assert.throws(() => crypto.createDecipheriv("des-ede3-cbc", "key", 10), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
console.log("crypto decipher argument validation passed");
