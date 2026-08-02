const assert = require("assert");
process.nextTick(
  (first, second) => {
    assert.strictEqual(first + second, "ab");
  },
  "a",
  "b",
);
