const assert = require("assert");

assert.deepStrictEqual(
  { first: 1, second: { left: true, right: false } },
  { second: { right: false, left: true }, first: 1 },
);
