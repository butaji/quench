const assert = require("assert");
const result = assert.throws(
  () =>
    assert.throws(() => {
      throw new TypeError({});
    }, assert.AssertionError),
  assert.AssertionError,
);
if (result.code !== "ERR_ASSERTION" || result.generatedMessage !== true) {
  throw new Error("missing assertion metadata");
}
