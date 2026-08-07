const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("http://nodejs.org/");
const resolved = parsed.resolveObject(
  "javascript:alert(1);a='@white-listed.com'",
);
assert.strictEqual(resolved.protocol, "javascript:");
assert.strictEqual(resolved.pathname, "alert(1);a='@white-listed.com'");
console.log("legacy URL resolveObject passed");
