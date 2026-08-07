const assert = require("assert");
assert.throws(() => assert.strictEqual({}, {}), {
  name: "AssertionError",
  code: "ERR_ASSERTION",
});
