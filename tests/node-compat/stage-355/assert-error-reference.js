const assert = require("assert");
assert.throws(() => assert.strictEqual(new Error("foo"), new Error("foobar")), {
  code: "ERR_ASSERTION",
  name: "AssertionError",
});
