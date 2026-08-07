const assert = require("node:assert");
const util = require("node:util");

assert.strictEqual(
  util.getSystemErrorName(-111111),
  "Unknown system error -111111",
);
assert.strictEqual(
  util.getSystemErrorMessage(-111111),
  "Unknown system error -111111",
);
assert.throws(() => util.getSystemErrorMessage("bad"), {
  code: "ERR_INVALID_ARG_TYPE",
});
