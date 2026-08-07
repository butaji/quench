const assert = require("assert");
assert.throws(() => assert.notStrictEqual("a ".repeat(30), "a ".repeat(30)), {
  message: `Expected "actual" to be strictly unequal to:\n\n'${
    "a ".repeat(30)
  }'`,
});
assert.throws(() => assert.notEqual(1, 1), {
  message: "1 != 1",
  operator: "!=",
});
