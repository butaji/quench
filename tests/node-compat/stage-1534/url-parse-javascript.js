const assert = require("node:assert");
const url = require("node:url");

const value = "javascript:alert(1);a='@white-listed.com'";
const parsed = url.parse(value);
assert.strictEqual(parsed.pathname, value.slice("javascript:".length));
assert.strictEqual(parsed.host, null);
assert.strictEqual(parsed.href, value);
console.log("javascript legacy URL parsing passed");
