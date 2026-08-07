const assert = require("assert");
assert.strictEqual(typeof process.cwd(), "string");
assert.strictEqual(process.cwd().length > 0, true);
assert.strictEqual(
  typeof process.env.PATH === "undefined" ||
    typeof process.env.PATH === "string",
  true,
);
