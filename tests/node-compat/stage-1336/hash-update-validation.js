const assert = require("node:assert");
const crypto = require("node:crypto");

assert.throws(() => crypto.createHash("sha256").update({}), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("hash update validation passed");
