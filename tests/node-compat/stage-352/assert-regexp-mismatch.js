const assert = require("assert");
assert.throws(
  () =>
    assert.throws(() => {
      throw Symbol("foo");
    }, /abc/),
  { code: "ERR_ASSERTION", operator: "throws" },
);
