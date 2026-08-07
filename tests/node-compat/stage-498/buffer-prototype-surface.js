const assert = require("assert");

assert.deepStrictEqual(
  Object.getOwnPropertyNames(Buffer.prototype).filter((name) =>
    name.startsWith("_")
  ),
  [],
);
assert.ok(Buffer.from([1]) instanceof Buffer);
