const assert = require("assert");
assert.throws(() => assert.notStrictEqual(2, 2), {
  name: "AssertionError",
  message: 'Expected "actual" to be strictly unequal to: 2',
});
