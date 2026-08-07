const assert = require("node:assert");
const { URLPattern } = require("node:url");

const pattern = new URLPattern();
assert.strictEqual(pattern.test(undefined, undefined), true);
assert.strictEqual(pattern.test("https://example.com", null), false);
assert.strictEqual(pattern.exec("https://example.com", null), null);
assert.throws(() => pattern.test(null, null), {
  code: "ERR_OPERATION_FAILED",
});
assert.strictEqual(
  new URLPattern("https://example.com", "https://example.com", null).hostname,
  "example.com",
);
console.log("URLPattern overloads passed");
