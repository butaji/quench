const assert = require("node:assert");

assert.throws(() => assert.fail(), {
  name: "AssertionError",
  message: "Failed",
  generatedMessage: true,
  operator: "fail",
});
assert.throws(() => assert.fail("custom message"), {
  message: "custom message",
  generatedMessage: false,
});
const original = new TypeError("custom error");
assert.throws(
  () => assert.fail(original),
  (error) => error === original,
);

const cause = new Error("test error");
assert.throws(() => assert.ifError(cause), {
  message: "ifError got unwanted exception: test error",
  actual: cause,
  expected: null,
  operator: "ifError",
});
assert.ifError(null);
assert.ifError(undefined);
console.log("assert fail and ifError passed");
