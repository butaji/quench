const assert = require("node:assert");

assert.throws(
  () => Object.defineProperty(process.env, "foo", { value: "foo1" }),
  { code: "ERR_INVALID_OBJECT_DEFINE_PROPERTY" },
);
assert.throws(
  () => Object.defineProperty(process.env, "goo", { get: () => "goo" }),
  { code: "ERR_INVALID_OBJECT_DEFINE_PROPERTY" },
);
Object.defineProperty(process.env, "valid", {
  value: "value",
  configurable: true,
  writable: true,
  enumerable: true,
});
assert.strictEqual(process.env.valid, "value");
