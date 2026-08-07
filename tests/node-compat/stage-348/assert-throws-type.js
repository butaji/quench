const assert = require("assert");
assert.throws(
  () =>
    assert.throws(() => {
      throw new TypeError("wrong");
    }, assert.AssertionError),
  assert.AssertionError,
);
