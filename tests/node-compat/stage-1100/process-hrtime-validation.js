const assert = require("node:assert");

assert.throws(() => process.hrtime(1), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => process.hrtime([]), { code: "ERR_OUT_OF_RANGE" });
assert.throws(() => process.hrtime([1, 2, 3]), {
  code: "ERR_OUT_OF_RANGE",
});
assert.strictEqual(process.hrtime([0, 0]).length, 2);
