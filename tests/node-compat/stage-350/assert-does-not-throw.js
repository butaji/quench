const assert = require("assert");
assert.throws(
  () =>
    assert.doesNotThrow(() => {
      throw new TypeError({});
    }, TypeError),
  { code: "ERR_ASSERTION", operator: "doesNotThrow" },
);
