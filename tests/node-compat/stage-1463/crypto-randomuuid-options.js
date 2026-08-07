const assert = require("node:assert");
const crypto = require("node:crypto");

assert.match(
  crypto.randomUUID({ disableEntropyCache: true }),
  /^[0-9a-f-]{36}$/,
);
assert.throws(() => crypto.randomUUID(1), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => crypto.randomUUID({ disableEntropyCache: "" }), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("crypto random UUID options passed");
