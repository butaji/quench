const assert = require("assert");
const { format } = require("util");

const value = Object.create(null);
value.answer = 42;
assert.strictEqual(
  format("%s", value),
  "[Object: null prototype] { answer: 42 }",
);
assert.strictEqual(
  format("%s", Object.create(null)),
  "[Object: null prototype] {}",
);
