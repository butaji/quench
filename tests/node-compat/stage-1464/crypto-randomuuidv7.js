const assert = require("node:assert");
const crypto = require("node:crypto");

const uuid = crypto.randomUUIDv7();
assert.match(
  uuid,
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
);
assert.throws(() => crypto.randomUUIDv7(1), { code: "ERR_INVALID_ARG_TYPE" });
console.log("crypto random UUIDv7 passed");
