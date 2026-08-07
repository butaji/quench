const assert = require("node:assert");
const { URLPattern } = require("node:url");

assert.throws(() => URLPattern(), { code: "ERR_CONSTRUCT_CALL_REQUIRED" });
assert.throws(() => new URLPattern(1), { code: "ERR_INVALID_ARG_TYPE" });
const pattern = new URLPattern({
  protocol: "https",
  hostname: "example.com",
  pathname: "/:id",
});
assert.strictEqual(pattern.protocol, "https");
assert.strictEqual(pattern.hostname, "example.com");
assert.strictEqual(pattern.pathname, "/:id");
assert.strictEqual(new URLPattern().protocol, "*");
assert.strictEqual(new URLPattern(null).hostname, "*");
console.log("URLPattern constructor surface passed");
